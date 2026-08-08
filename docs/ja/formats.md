# フォーマット

## 組み込みアダプター

`attx formats` が、id・拡張子・入力がファイルかディレクトリかを JSON で出力する権威ある一覧です。検出順序は固定；JSON の `.json` 系はコンテンツで嗅ぎ分けられ、最も特定性の高いものから試されます。

| id | 拡張子 | 入力 | 出力 |
|----|-----------|-------|--------|
| `rmmz` | — | ディレクトリ | インプレース + `*.attxbak` |
| `epub` | `.epub` | ファイル | `<name>.<dst>.epub` |
| `html` | `.html` `.htm` `.xhtml` | ファイル | 翻訳済みコピー |
| `docx` | `.docx` | ファイル | `<name>.<dst>.docx` |
| `xlsx` | `.xlsx` `.xlsm` | ファイル | 翻訳済みコピー |
| `srt` | `.srt` | ファイル | 翻訳済みコピー |
| `vtt` | `.vtt` | ファイル | 翻訳済みコピー |
| `ass` | `.ass` `.ssa` | ファイル | 翻訳済みコピー |
| `lrc` | `.lrc` | ファイル | 翻訳済みコピー |
| `csv` | `.csv` `.tsv` | ファイル | 翻訳済みコピー |
| `po` | `.po` `.pot` | ファイル | 翻訳済みコピー |
| `renpy` | `.rpy` | ファイル | 翻訳済みコピー |
| `md` | `.md` `.markdown` | ファイル | 翻訳済みコピー |
| `txt` | `.txt` | ファイル | 翻訳済みコピー |
| `paratranz` | `.json`（嗅ぎ分け） | ファイル | 翻訳済みコピー |
| `vnt` | `.json`（嗅ぎ分け） | ファイル | 翻訳済みコピー |
| `mtool` | `.json`（嗅ぎ分け） | ファイル | 翻訳済みコピー |
| `i18next` | `.json`（嗅ぎ分け） | ファイル | 翻訳済みコピー |
| `jsonl` | `.jsonl`、または `source.jsonl` のあるディレクトリ | ファイルまたはディレクトリ | `translated.jsonl` |
| `custom:<name>` | プロファイル由来 | ファイルまたはディレクトリ | コピー、`overwrite = true` ならインプレース |

検出があいまいな場合や誤っている場合に特定のアダプターを強制：`attx init --engine <id>`。

### フォーマット別メモ

- **epub** — リーフブロック（`p`、見出し、`li`、…）上の段落単位のユニット；ルビの読み（`<rt>`/`<rp>`）はソーステキストから除去されます；画像とレイアウトは保持；writeback で `dc:language` が更新されます。
- **docx** — `w:t` ラン（本文 + 脚注/文末脚注）上の段落単位；各段落の最初のランが翻訳を受け取ります。
- **xlsx** — 共有文字列テーブル（`xl/sharedStrings.xml`）を翻訳するため、全シートで一貫します；発音用の `rPh` ランはスキップされます。
- **srt/vtt/lrc** — タイミング行、ヘッダー、メタデータはそのまま；キュー/歌詞テキストのみ翻訳されます。
- **ass** — `Dialogue:` の Text フィールドのみ；`{\tag}` オーバーライドと `\N` 改行は保持；`Name` 列が話者ロールになります。
- **csv/tsv** — セル単位のユニット（RFC 4180：引用符付きフィールド、埋め込み改行）；ソース言語テキストのあるレコードのみ再レンダリングされます。
- **po** — `msgstr` を埋めます；ヘッダーエントリと `msgid_plural` エントリはそのままパススルーされます。
- **renpy** — `translate` ブロック内のみ：引用符付きダイアログと `old`/`new` 文字列ペア；アセット文（voice/play/show/…）はスキップされます。
- **rmmz** — [RPG Maker](rmmz.md) を参照。
- **mtool/paratranz/vnt/i18next** — コンテンツ嗅ぎ分けによる JSON 形状（MTool `ManualTransFile.json`、空の `translation` フィールドのみを埋める Paratranz エクスポート、VNTextPatch の `name`/`message`、i18next のネストされた文字列リーフ）。
- **jsonl** — エスケープハッチ：外部の抽出/書き込みスクリプトで任意のエンジンを使用可能；抽出時にソース言語のフィルタリングはありません。

### エンコーディング

テキスト入力はエンコーディングを自動検出します：厳格 UTF-8 → UTF-16（BOM）→ `chardetng` の推測（Shift-JIS、GBK、…）→ `encoding_rs` でのデコード。出力は**常に UTF-8** です。

## 未知のフォーマット？attx にプロファイルを教える

```bash
attx analyze --input ./project         # 偵察：エンコーディング、構造、サンプル、JSON 形状
attx profile new --output fmt.toml     # ドキュメント付きルールテンプレート
attx profile test --profile fmt.toml --input ./project --roundtrip   # 反復
attx init --input ./project --profile fmt.toml --src ja --dst zh
attx profile save --profile fmt.toml   # 以降 detect が自動認識
```

### プロファイルスキーマ

```toml
name = "myformat"                    # id → engine "custom:myformat"
label = "My format"
extensions = ["ks"]                  # 例 ["ks", "scn"]
detect_regex = []                    # 最初の 64 KiB で ALL が一致する必要がある
min_units = 1                        # 自動検出にはこの数以上のユニットが必要
overwrite = false                    # true = インプレースで書き戻す
skip_lines = []                      # line_regex モード：一致する行をスキップ
notes = ""

# 行単位の正規表現：(?P<text>...) 必須、(?P<role>...) 任意
[[rules]]
kind = "line_regex"
pattern = '^(?P<role>[^\s@;]*)\s*「(?P<text>.+)」$'

# JSON：これらのオブジェクトキーの下の文字列値（任意の深さ）
[[rules]]
kind = "json_keys"
keys = ["message", "name"]

# JSON：パスグロブの文字列リーフ（* は 1 階層、** は任意の深さ）
[[rules]]
kind = "json_paths"
paths = ["events/*/text", "**/choices/*"]
```

保存済みプロファイルは `$ATTX_HOME/profiles/`（または `~/.config/attx/profiles/`）に置かれ、`attx formats` / `attx detect` で `custom:<name>` として表示されます。

例：`profiles/examples/`（KiriKiri KAG、INI、汎用 JSON）。エージェントワークフロー：`skills/attx/references/custom-format-discovery.md`。
