# ワークスペース、キャッシュ、JSONL

ワークスペースは、入力・engine・原文言語・対象言語に結び付いたローカル作業状態です。SQLite は翻訳の進捗を保持しますが、ゲームや文書そのものの完全バックアップではありません。

## 保存ファイルと同一性

| ファイル | 役割 |
|---|---|
| `attx.db` | source unit、source hash に結び付いた translation、metadata |
| `workspace.json` | input、content_root、engine、語向などの可読 snapshot |
| `.attx.lock` | 同時に変更するコマンドを防ぐ OS lock の対象 |
| `profile.toml` | custom profile の immutable snapshot。利用時のみ |
| `glossary.toml` | active/rejected の語彙表。作成時のみ |
| `preserve.toml` | 追加 literal 保護 rule |
| `experience.toml` | 作品 scope の抽出経験・prompt note |

directory の既定位置は検出した `<content_root>/.attx`、file は `<parent>/.attx-<stem>` です。stem が同じ別拡張子を同じ親に置く場合も、workspace identity は別入力を拒否するため、`--workspace` に別 directory を指定します。

```bash
attx init --input novel.epub --src ja --dst zh --workspace ./work/novel-ja-zh
```

既存 workspace を別 input、engine、語向に使うと init は拒否します。profile 内容は作成時の snapshot/digest で固定され、コピーした profile の overwrite flag の手変更も拒否します。別条件には新しい workspace が必要です。metadata や DB を手編集してこの条件を外さないでください。

## ロックと再開

変更コマンドは workspace の `.attx.lock` に OS lock を取ります。同一 workspace で同時に複数の translate、repair、import、writeback を走らせないでください。別プロセスが使用中なら busy を返します。クラッシュ後は OS が lock を解放するため、lock ファイルが残ること自体は stale lock ではありません。動いているプロセスを調べずにファイルを消さないでください。

`run` は抽出、語彙表、翻訳、書き戻しを通して一つの workspace lock を保持し、途中の段階へ別の変更コマンドが割り込むことを防ぎます。

成功した翻訳は incremental に保存されます。プロセスを止めてもコミット済み cache は再利用できます。ただし送信済みで未保存の HTTP は、provider では課金済みかもしれません。

```bash
attx status --workspace ./work/novel-ja-zh
attx translate --workspace ./work/novel-ja-zh
```

pending は有効な対応訳がない項目、passthrough は原文の仮置きです。後者を成功と合算しません。review は残留や構造異常も検出するので pending=0 だけで完了判定しないでください。

## 原文 hash と anchor

TextUnit は location、engine、original_lines と stable ID を持ちます。通常 ID は engine・location・原文から計算し、translation は source_hash を保存します。抽出更新では今の原文に一致する cache を残し、変更・消失した原文へ古い訳を適用しません。

writeback は再抽出した原文の位置と hash が cached unit に一致するか検査します。抽出時に保存した全ユニット集合の snapshot とも比較し、元項目の変更だけでなく新しい本文の追加・削除も拒否します。行を挿入して位置をずらしたファイル、ゲーム update、別版の JSONL はそのまま旧訳へ結び付けられません。今の原文を正しく extract するか、新 workspace で処理してください。

SQLite の `published` table は実際に出力した行を記録します。custom overwrite を再実行するとき、元の source と以前に公開した訳を区別して JSONL 修正を反映し、無関係な live key/comment は保持します。live source の任意変更をすべて許可する仕組みではなく、source/anchor と公開履歴を検証します。

内部 metadata の `output_paths` は実際に生成したファイルを記録します。custom directory profile はその記録済み副本だけを除外し、言語 suffix が付いた本物の原文を黙って捨てません。副本の出力先が別の原文と衝突すると拒否します。古い workspace に anchor snapshot がない場合は `extract --workspace` で更新でき、ID/hash が一致する cache は保持します。

RMMZ 原文ソースは data_origin、次に `.attxbak` を優先し、live data へ出力します。overwrite profile も既存 backup を原文経路に使う場合があります。原文として使う backup と live file の差を理解し、翻訳出力だけを次の原文として扱わないでください。

## 書き戻しと backup

既定では pending、passthrough、無効な構造、残留などの未解決項目があれば出力を block します。`--allow-partial` はユーザーが部分出力を明示的に受け入れた場合だけ使い、有効な subset を適用して他は原文を保持します。repeat RMMZ writeback でも無効項目を抽出原文へ戻し、以前 live にあった旧訳を黙って残しません。結果は needs_attention、終了 2 です。

artifact は全件 stage した後、既存 target の backup を検査して各 target を置換します。バックアップ名は `file.ext.attxbak` で、初回の既存内容を保持し、実行ごとに更新しません。backup の失敗や不正な backup target は置換を止めます。temporary は同じ directory で作られます。

出力と完成した backup の permissions は source から引き継ぎます。temporary は Unix では0600で作成し、cap-stdの開いたdirectory handleに結び付けて生成・置換します。same-fileで親directoryの同一性も確認し、機密sourceのpermissionsを勝手に公開範囲へ広げません。

置換は 1 ファイル単位で atomic です。ディレクトリ全体の transaction ではないので、途中の filesystem エラーでは既に置換済みのファイルがあり得ます。レポートを確認し、重要な入力には別の完全コピーも持ってください。

writeback は Chinese 正規化と RMMZ fixed-slot reflow をメモリ内で検討します。実際に成功した書き戻しで変更訳を cache に同期します。dry-run と blocked は出力も cache も変更しません。`writeback --dry-run` の files/paths は計画であり、実ファイルの生成証拠ではありません。

## JSONL の schema

各行が独立 JSON object です。最低 id と text を持ち、multi-line text は文字列内の `\n` です。

```json
{"id":"scene01:55","text":"こんにちは。","context":"scene01","role":"アレイ","item_type":"short_text"}
```

| フィールド | 用途 |
|---|---|
| `id` | stable source location。workspace export は unit location を使う |
| `text` | その ID の完全な原文。訳文で置き換えない |
| `context` | 任意。scene の grouping |
| `role` | 任意。話者など |
| `item_type` | 任意。long_text、array、short_text |
| `translation` | 任意。multi-line の訳文字列 |
| `translation_lines` | 任意。訳の行配列。存在時はこちらを優先 |

## workspace のない直接翻訳

```bash
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh
```

外部 extractor の id/text を翻訳し、原文 identity と translation/translation_lines を出力します。これは workspace による incremental resumption と repair loop を持たず、Translator 内の有限 retry のみです。不完全なら未解決を可視化したレポートと終了 2 を返します。出力は stage して置換し、既存 output は backup します。

元の binary や未知 engine に安全に戻すのは、形式を理解した外部 writeback tool の仕事です。attx がその engine の書き戻し能力を自動的に獲得するわけではありません。モデル生成 extractor や writer を無条件に実行しないでください。

## JSONL workspace

cache と review/repair が必要なら `.jsonl` を input にするか、directory に `source.jsonl` を置きます。

```bash
attx run --input ./source.jsonl --engine jsonl --src ja --dst zh
attx run --input ./external-project --engine jsonl --src ja --dst zh
```

file は `<stem>.<dst>.jsonl`、directory は source.jsonl の隣の translated.jsonl が出力です。JSONL adapter は source-language filter を行いません。stable ID の重複、別版原文の混入を避けます。

## export、修正、import

```bash
attx export-jsonl --workspace ./work/novel-ja-zh --output review.jsonl --filter all
# id と text を保持して translation のみを修正
attx import-jsonl --workspace ./work/novel-ja-zh --input review.jsonl
attx review --workspace ./work/novel-ja-zh
attx writeback --workspace ./work/novel-ja-zh
```

filter は pending (既定)、all、translated、passthrough。export の ID は location です。import は現在の location または内部 unit ID を解決できますが、text が完全に今の原文と一致しなければいけません。

全行を先に検証し、通過後に transaction で保存します。次は全 import を拒否します。

- 不明 ID、原文 mismatch、壊れた record。
- 同じ unit へ解決される重複 record。location と内部 unit ID をそれぞれ使っても重複です。
- 空でない訳の行構造違反、control/variable 損失など。

校正 export の空または欠落した訳は skip し、imported に数えません。Chinese の機械端片は正規化して保存し、mixed/untranslated は後続 repair が必要な意味的残留として保持できます。import が成功しても、default writeback の完全性条件が解除されるわけではありません。

API キーを JSONL、metadata、report に入れません。全文の監査には export を使い、最大 40 件の review sample を全問題一覧と見なさないでください。

関連: [使い方](usage.md)、[品質と修復](quality.md)、[CLI](cli.md)。
