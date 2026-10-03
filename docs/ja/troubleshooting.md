# トラブルシューティング

最終 stdout JSON、終了コード、stderr のエラーを分けて保存します。API key、private config 全体、機密 source を公開 issue に貼らないでください。普通の失敗を DB 削除、原文手編集、上限なしの retry で解決しないことが基本です。

## API と設定

| 症状 | 確認と対処 |
|---|---|
| `no LLM clients configured` | setting.toml の検索先と [llm]/[[llm.clients]] を確認。非 LLM コマンドなら設定なしでも可 |
| `llm client not found` | default_client または --client が clients.name と一致するか確認 |
| doctor が exit0 でも接続できない | llm.configured と ping の内容を見る。doctor のトップレベル ok は接続成功とは別 |
| 401/403 | ローカルの key、権限、provider project を修正。恒久的な認証失敗は即停止し、繰り返し ping/retry しない |
| 400/404 など | base_url が /chat/completions まで含まれていないか、model 名、extra、対応 API を確認 |
| 429 | worker_count と rpm を provider の quota に合わせて下げる。自動 retry は有限 |
| timeout/network/5xx | 通信・provider 状態を確認。timeout は最低 30 秒、既定 600 秒。batch と出力上限も検討 |
| SSE が parse できない | stream=false で互換 JSON 応答を使うか、provider の delta.content SSE 対応を確認 |
| response truncation | max_tokens/max_completion_tokens、batch_chars、items を確認。切断応答を成功としない |

client を複数設定しても自動 key rotation はありません。fatal error は queue 中の新規要求を止めますが、in-flight 要求は provider 側で完了・課金される場合があります。成功済み cache を捨てずに原因を直します。

## 形式と抽出

```bash
attx formats
attx detect --input ./input
attx analyze --input ./input --src ja
```

| 症状 | 確認と対処 |
|---|---|
| detect が一致しない | analyze で text/binary と structure を確認。run は safe profile inference を試せる。禁止は --no-infer |
| binary/暗号化/曖昧な構造 | 既知 external extractor で JSONL を作る。モデル生成 parser を無条件に実行しない |
| extracted zero units | source 語向、content root、engine、profile pattern、machine literal filter を確認。run はゼロ抽出で止まる |
| .json が別形式になった | formats/detect の content sniff を確認し、形式を理解して init/run --engine を選ぶ |
| profile test がゼロ | extension、detect_regex、named text capture、JSON key/path を確認 |
| auto directory が全部翻訳されない | auto_coverage を読む。archive/字幕/script の専用 dispatch ではなく、未対応 file はコピーする |
| text が文字化け | analyze の encoding と lossy signal を確認。原文 encoding を正しく用意し、強制 prose で回避しない |
| auto が target encoding を表せない | 元 codec の制約。勝手に UTF-8 化せず、対応 codec の input や専用変換経路を用意 |

profile infer の no-op roundtrip 失敗は安全性拒否です。manual rule の調整または専用 adapter に切り替えます。検証を省略した無理な profile 保存で回避しません。

## キャッシュと source 同一性

| 症状 | 確認と対処 |
|---|---|
| workspace belongs to different input/engine/language | --workspace を新しい directory にする。別作品 DB を借りない |
| workspace profile changed / digest mismatch | 元 snapshot と同じ rule を使う。変更条件は新 workspace。overwrite flag の手編集で回避しない |
| workspace busy | 同じ workspace の変更 process が終わるまで待つ。run は処理全体で lock を保持する |
| crash 後 .attx.lock がある | file の存在だけは lock 残留ではない。OS lock は crash で解放される |
| source changed / anchor mismatch | 現原文を確認。追加・削除された抽出 unit も含め、旧 cache を別版へ writeback しない |
| 再実行しても完了項目が変わらない | source 一致 cache を再利用する仕様。新 note/model は既訳を自動全量再翻訳しない |
| 学習で表示 text が消えた | learn list/defaults の evidence を確認。原因が filter と特定できれば extract --no-knowledge を比較用に使う |

RMMZ は data_origin、次に `.attxbak` を原文ソースにし、live の data/plugins.js へ出力します。原文ソースを維持したい場合、翻訳済み live file を source として再登録しません。復元・workspace 削除・reset は対象を確定した別操作です。

## exit2 と未解決項目

```bash
attx status --workspace .attx-input
attx review --workspace .attx-input
attx repair --workspace .attx-input --dry-run
attx repair --workspace .attx-input
```

exit2 は翻訳完了ではなく、structured report を読んで処理する状態です。pending、passthrough、unresolved、review bucket、skipped_note を見ます。dry-run は計画のみで 0、argument parse の 2 は結果 JSON と区別します。

- passthrough は原文仮置きです。translate --retry-passthrough または有限 repair を使います。
- duplicate/omitted IDs、line mismatch、control loss は model contract 違反。prompt と batch 規模を確認します。
- kana_edge は Chinese context の機械端片、kana_mixed/untranslated は本当に残る日本語の signal。本文を削って count を下げないでください。
- glossary advisory は substring の助言。語形変化を読み、advisory だけで無限 retry しません。
- limit を設定した実行は全量ではありません。未翻訳残りがあれば translate で同 workspace を続行します。

有限 repair が進展しない場合は JSONL で具体的な訳を修正します。review sample は各 40 件までなので、全件には export を使います。

```bash
attx export-jsonl --workspace .attx-input --output review.jsonl --filter all
# 訳欄のみ修正
attx import-jsonl --workspace .attx-input --input review.jsonl
attx review --workspace .attx-input
attx writeback --workspace .attx-input
```

## 書き戻しと表示

| 症状 | 確認と対処 |
|---|---|
| writeback blocked | pending/invalid/residual を解決する。追加 permission の不足ではない |
| 部分 output が必要 | ユーザーが原文残留を受け入れた場合のみ --allow-partial。有効 subset を書き、exit2 を保持 |
| backup failure | permission、disk、backup path が普通 file か確認。backup を勝手に削除して上書きしない |
| dry-run 後に file がない | 予定だけの仕様。実 writeback して paths を読む |
| 途中 I/O failure | per-file atomic で directory transaction ではない。既に書いた file と backup を確認して復旧 |
| RMMZ speaker が不自然 | namebox_mismatch と export の role/訳を確認。第 1 speaker slot は保護される |
| RMMZ overflow_lines がある | 元 command/slot 数を保持するため、最後の slot に長い残りがあり得る。訳を簡潔に調整し再 import、ゲーム表示確認 |
| plugin に漏れがある | plugins.js の対象表示 parameter を確認。plugin source は意図的に対象外 |
| 文書の装飾が変わる | DOCX run 集約など adapter 固有の範囲を確認。機械 ok は renderer の見え方を保証しない |

repeat RMMZ partial writeback でも無効 unit は今の抽出原文へ戻し、過去に live にあった旧訳を残して成功に見せません。Chinese 正規化と slot reflow の成功結果は cache と同期されるため、output だけを手で直さないでください。

## JSONL import の拒否

unknown ID、text mismatch、同一 unit の重複 alias、invalid structure/control は全 import 拒否です。途中まで保存する方式ではありません。今の workspace から再 export し、id/text を維持して訳だけ修正します。location ID と internal hash ID を同じ unit に二度使わないでください。空の監査訳は skip され、imported に数えません。

## 問題を報告する

binary version、OS、engine、source/target、実際の command (秘密を除く)、終了コード、短いエラー、最小の匿名 fixture、期待出力を示します。source と output の差、dry-run と実行のどちらか、cache を再利用したか、coverage/overflow を明記すると原因を絞れます。API key、private file、実ゲーム全文の公開を診断の前提にしません。

関連: [設定](configuration.md)、[CLI](cli.md)、[品質と修復](quality.md)、[ワークスペース](workspace.md)。
