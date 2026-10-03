# 品質、正規化、有限修復

attx は構造の破損と目に見える未翻訳を機械的に検出します。意味の正しさ、口調、作品内文脈のすべてを判定する翻訳審査員ではありません。品質レポートと実際の読み直し・ゲーム内確認を併用してください。

## モデル応答の検証

原文ユニットは stable ID、行配列、role、scene とともに送ります。制御コード、変数、markup の対象は `[CTRL_n]` に mask し、返答後に復元します。すべての保護 literal は出現数を保持し、engine control、markup、暗黙の位置 printf 引数は相対順序も保持します。名前付きまたは明示 index の placeholder は対象言語の語順に合わせて移動できます。

- 不明 ID は採用しません。欠落 ID は失敗項目です。
- 同じ ID の重複は成功件数を増やしません。最初の検証済みの有効行を一度だけ採用し、後の重複で上書きしません。不正行は独立した有効行を失敗にしません。
- 切断された応答、行配列の不一致、mask の欠落・重複・改変は成功扱いしません。
- 成功済み項目は再送せず、失敗項目を縮小して再試行します。
- 試行を使い切った場合は原文を passthrough の仮置きとして保存し、成功件数と分けます。

`retry_count` は Translator 1 パス内の上限、`repair_rounds` は追加の機械レビュー修復パスの上限です。恒久的な HTTP 4xx を小分けして何度も送信しません。詳細は[設定](configuration.md)にあります。

## context と翻訳指示

原文順は location の自然な数値順を考慮し、file/scene 境界で分けます。前後原文とコミット済み訳文は `context_chars` の合計予算内で注入します。語彙表はバッチに実際に出る語を上限付きで使い、承認済み `topic=prompt` note を補助指示として入れます。

既定 system prompt は対象言語、原文意味、speaker、line count、control preservation、出力形式を指示します。ただし prompt があるだけでモデルの遵守を保証しません。文体を変えるには作品 scope の `learn note` を使い、固有名詞は glossary に置いてください。設定変更は既に成功したキャッシュを自動で全量再翻訳しません。

## 中国語ターゲットの正規化

中国語の本文である十分な手掛かりがある場合だけ、機械的な日本語の小片を処理します。

| パターン | 中国語文脈での処理 |
|---|---|
| 孤立した `っ`、`ッ` | 削除 |
| 機械的な `っすよ`、`っす` | `哦` に置換 |
| 長音 `ー` | `～` に置換 |
| 中黒 `・` | 区切りとして保持 |

保護済み literal、制御コード内の仮名、日本語の全文、引用して形を示している文字を機械的に削除しません。例えば日本語片を含む説明や、本当に翻訳されていない文は残留として報告します。すべての仮名を消して未訳を隠す機能ではありません。

正規化はモデルの結果、JSONL import、既存 cache の経路で共通の規則を使います。成功した実際の writeback は変更された訳を cache に同期し、次回の export と review が出力と同じ訳を見るようにします。dry-run と blocked writeback は正規化をメモリ内で検討しても、cache や出力を変更しません。

## レビューの分類

```bash
attx review --workspace .attx-novel
```

API を呼ばず、各 bucket に `count` と最大 40 件の `sample` を返します。sample は `location`、`unit_id`、`detail` です。

| bucket | 意味 |
|---|---|
| `residual_source` | 原文言語が残っている集約 signal。identical や passthrough の原文も含む |
| `kana_edge` | 中国語に付いた機械的な仮名の端片 |
| `kana_mixed` | 中国語などの対象本文に日本語仮名が混在 |
| `kana_untranslated` | 日本語片が未翻訳、または必要な原文のコピー |
| `identical` | 翻訳が必要なのに visible 原文と同一 |
| `control_loss` | 保護された control・変数の損失 |
| `namebox_mismatch` | 姓名框と本文先頭の話者名関係の不一致 |
| `glossary.violations` | 指定訳名の substring が見当たらない助言 |

3 種の kana 分類は同一項目に対する排他的な分類です。一方 residual_source、identical、control_loss などの集約 bucket とは重複できます。全 count を足して unique 問題数にしないでください。sample の 40 件上限は修復対象の上限ではありません。

原文言語検出は Latin/CJK/Hangul/Cyrillic/Arabic/Devanagari/Thai などの script を使う heuristic です。厳密な言語同定ではなく、共有する script や短い名前では誤判定し得ます。Chinese の中の漢字だけで日本語を完全に識別することもできません。

日本語と中国語で同じ漢字 label、例えば `魔法` や `通信` は、文字が変わらないことだけで失敗にしません。正しい対象 label を無限に言い換えることを避けますが、共有 script の意味が適切かは別途確認が必要です。

語彙 check は substring の審査補助です。語形変化や構文上の自然な変更で指定文字列がそのまま出ない場合があり、advisory だけで writeback を拒否したり無限に再翻訳しません。

## 修復と停止条件

```bash
attx repair --workspace .attx-novel --dry-run
attx repair --workspace .attx-novel
```

pending、passthrough、残留原文、構造・制御損失、話者不一致を有限に処理します。translate/run にもレビュー後の自動修復があります。明示的 `repair` は repair_rounds=0 でも最低 1 パスを試します。上限に達したか進展がなくなれば未解決を報告します。同じ外側コマンドを無制限に繰り返してこの上限を打ち消さないでください。

`pending=0` だけでは品質完了ではありません。TranslateReport の `unresolved`、`passthrough`、`review`、`skipped_note` を確認します。不完全な非 dry-run は JSON を出して終了 2 です。

## 手修正と部分出力

```bash
attx export-jsonl --workspace .attx-novel --output review.jsonl --filter all
# id と text を維持し、translation_lines または translation のみ修正
attx import-jsonl --workspace .attx-novel --input review.jsonl
attx review --workspace .attx-novel
attx writeback --workspace .attx-novel
```

既定では pending や invalid が残れば出力を block します。ユーザーが未解決原文を含む出力を受け入れたときだけ `writeback --allow-partial` または `run --allow-partial` を使います。有効な訳のみ適用し、その他は原文を保ち、結果は needs_attention と終了 2 です。

RMMZ は固定スロットへ再配置し、`reflowed_units` と `overflow_lines` を報告します。[RPG Maker](rmmz.md)と[ワークスペース](workspace.md)で source hash、backup、cache 同期を確認してください。
