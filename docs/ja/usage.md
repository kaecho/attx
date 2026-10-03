# 日常の使い方

初回設定は[クイックスタート](quickstart.md)、すべてのフラグは[CLI](cli.md)を参照してください。通常入口は `run` ですが、抽出・翻訳・書き戻しを分けても同じ SQLite cache を使います。

## 一括実行と段階実行

```bash
attx run --input novel.epub --src ja --dst zh
```

run は init、extract、設定で有効な glossary、translate、有限 repair、writeback をまとめます。未対応 text には必要に応じて profile infer を行います。`--no-infer` は有料推論を禁止します。`--no-translate` は抽出と確認のみ、`--no-writeback` は翻訳 cache までに止めます。

段階を制御する場合:

```bash
attx init --input novel.epub --src ja --dst zh
attx extract --workspace .attx-novel
attx translate --workspace .attx-novel
attx review --workspace .attx-novel
attx writeback --workspace .attx-novel
```

preview や限額が必要なら `translate --dry-run`、`--limit N`、`writeback --dry-run` を追加します。必ず試訳をして追加許可を求める仕組みではありません。`--limit` で全体を残したまま普通の writeback をすれば完全性検査で block され得ます。

## 再開と修復

```bash
attx status --workspace .attx-novel
attx translate --workspace .attx-novel
attx repair --workspace .attx-novel
```

同じ input・engine・語向の workspace を再利用します。コミット済みかつ source hash が一致する訳は維持します。pending は有効な cache がないもの、passthrough は要求失敗などで原文を保存した仮置きです。`translate --retry-passthrough` はこれらを再 queue し、共有 pipeline は機械レビュー問題も有限に処理します。

無限の外側再試行は避け、上限または進展停止後は[品質と修復](quality.md)の JSONL 修正へ進みます。新しい model や note を選んでも、成功済みの全項目を自動で再翻訳はしません。

## 語彙表

固有名詞の訳を作品内で揃えるために glossary を使います。構築は追加 API 呼び出しを使い、既定で無効です。既に有効な設定や明示的な依頼があればその範囲で実行できます。

```bash
attx glossary build --workspace .attx-novel --dry-run
attx glossary build --workspace .attx-novel
attx glossary list --workspace .attx-novel
attx glossary add --workspace .attx-novel --src "アレイ" --dst "艾蕾" --info "女性の話者名"
attx glossary check --workspace .attx-novel
```

model が実原文から候補を挙げ、原文 substring とその語を含むユニット数を検証して保持します。複数行ユニットで何度出ても 1 件です。既定は min_occurrences=10、max_terms=200、inject_limit=30、最大 40 抽出 batch です。姓名框の名前には専用処理があります。候補が保持数上限を超えれば `truncated` を報告します。40 batch の原文 coverage 上限は別の限界であり、語彙表があるだけで固有名詞の全使用を保証しません。

保存 schema は `glossary.toml` です。

```toml
version = 1
source_lang = "ja"
target_lang = "zh"

[[term]]
src = "アレイ"
dst = "艾蕾"
info = "女性の話者名"
count = 12
status = "active"
source = "manual"
case_sensitive = false
```

`src` は原語、`dst` は訳名、`info` は曖昧さを減らす説明、`count` は出現の証拠、`status` は active または rejected、`source` は由来です。英語の `May` と `may` などを区別する場合に `--case-sensitive` を使います。rejected は再構築で同じ非用語を繰り返し有料確認しないため保持し、`list --all` で見られます。

```bash
attx glossary import --workspace .attx-novel --file terms.json
attx glossary export --workspace .attx-novel --file terms-export.json
attx glossary remove --workspace .attx-novel --src "アレイ"
```

import は `[{"src":"アレイ","dst":"艾蕾","info":"女性名"}]` または `{"アレイ":"艾蕾"}` を受け付けます。check の violations は substring による助言で、語形変化もあるため自動 hard block の根拠ではありません。

## 保護する literal

送信前に regex の一致箇所を `[CTRL_n]` にして、応答後に元 literal を戻します。内蔵規則は RMMZ control、brace 変数、printf 形式、literal mask token、Ren'Py interpolation、対象形式の markup などです。重複 span は左端優先、同じ始点なら長い一致を使います。

```bash
attx preserve list --workspace .attx-novel
attx preserve add --workspace .attx-novel --pattern '<PLAYER_[0-9]+>' --info '固定の player token'
attx preserve remove --workspace .attx-novel --pattern '<PLAYER_[0-9]+>'
```

PowerShell でも regex は単一引用符で囲めます。空に一致する regex と不正 regex は拒否します。任意に本文全体を保護して漏翻を隠さないでください。

workspace の `preserve.toml`:

```toml
version = 1
[[rule]]
pattern = '<PLAYER_[0-9]+>'
info = "固定 token"
```

list には pattern、info、builtin/workspace の source を表示します。内蔵 rule は workspace の remove で無効にはしません。

## 文体メモと抽出経験

翻訳スタイルは具体的な note にします。

```bash
attx learn note --workspace .attx-novel --name voice --text "主人公は短い口語で話し、地の文は落ち着いた文体にする。"
attx learn list --workspace .attx-novel
```

`topic` の既定値 prompt だけが次の翻訳 system prompt に入ります。他 topic は人間用の記録です。`name` が同じなら更新し、別名なら積み重ねます。固有名詞の対応は note ではなく glossary に置きます。

`--workspace` は作品内の `experience.toml`、`--format rmmz` は将来の同形式作品に使う global knowledge です。scope を不用意に広げないでください。

経験 schema は version=2 で `[[entry]]` を使います。

```toml
format = "rmmz"
version = 2

[[entry]]
kind = "field"
field = "resourcekey"
verdict = "skip"
scope = "nested"
domain = "plugins"
status = "pending"
confidence = 0.9
reason = "本文ではなく resource の identity"
evidence = ["workspace の具体的な位置と hit 数"]
source = "summary"
updated_at = ""

[[entry]]
kind = "note"
name = "voice"
topic = "prompt"
text = "役割ごとの語調を維持する。"
status = "approved"
source = "manual"
updated_at = ""
```

field は小文字の名前で、先頭 `*` は suffix match。verdict は skip/extract、scope は top/nested/any、domain が空なら全 domain、status は pending/approved です。confidence、reason、evidence、source、updated_at は由来・判断の記録です。未知 kind と追加情報は保持されますが、未知 kind を実行することはありません。旧 version=1 の `[[rule]]` も読み込み、書き出しは version=2 です。

適用順は embedded default、global format knowledge、workspace override。後の layer が優先し、同 layer は exact field が suffix より優先、同条件なら skip が優先します。承認済みだけを使います。

経験適用は抽出済みユニットへの除外処理です。extract verdict は他 layer の skip を上書きできますが、アダプターが作らなかったユニットを新しく発明しません。機械 literal を本文へ復活させるルールでもありません。

## まとめと破壊的提案の承認

正常 writeback 後は既定で既存証拠をまとめます。通常 API 費用はありません。`--no-learn` または `[learn].auto_summarize=false` で省略できます。

```bash
attx learn summarize --workspace .attx-novel
attx learn pending
attx learn review --approve 1,3 --reject 2
attx learn list --format rmmz
attx learn forget --field resourcekey --format rmmz
attx learn forget --name voice --workspace .attx-novel
```

`scan` は summarize の別名です。`--llm` または llm_review=true は追加料金の審査です。pending skip は抽出本文を削るため、証拠を読んでユーザーが 1-based index で承認します。CLI に `--approve-all` はありますが、エージェントが独断で使ってはいけません。翻訳・通常 writeback の既存許可と、この破壊的承認は別です。

学習 filter が原因と特定できた場合は `extract --workspace .attx-novel --no-knowledge` を escape hatch として使います。正常診断の代わりに全 rule を毎回無視しないでください。

## 外部レビューと出力

```bash
attx export-jsonl --workspace .attx-novel --output review.jsonl --filter all
attx import-jsonl --workspace .attx-novel --input review.jsonl
attx writeback --workspace .attx-novel
```

id と text を維持し、translation のみを編集します。import の全件検証、source hash、lock、backup、出力規約の詳細は[ワークスペース](workspace.md)にあります。
