# CLI

`attx [--config <path>] [--client <name>] <command> [options]`

成功 → stdout に JSON、終了コード 0。失敗 → stderr に `error: …`、ゼロ以外の終了コード。

stdout の JSON 形状の正確な仕様は `skills/attx/references/cli-command-contract.md` に固定されています — エージェントはそのファイルと、現在のバイナリの `--help` に依存すべきです。

## グローバルオプション

| オプション | 意味 |
|--------|---------|
| `--config <path>` | `setting.toml` へのパス（デフォルト：`$ATTX_HOME/setting.toml` → `./setting.toml`） |
| `--client <name>` | `[llm].default_client` の代わりにこの `[[llm.clients]]` エントリを使う |

`--input` は別名 `--game` も受け付けます。

## コマンドリファレンス

### `doctor [--ping] [--json]`

設定と、任意で LLM の接続をチェック。通常出力は人間可読；`--json` は `{"llm":{configured,error},"ping","adapters":[],"saved_profiles":[],"status"}` を出力します（API キーは決して含まれません）。

### `formats`

対応アダプターと保存済みプロファイルを JSON で一覧表示：`{"formats":[{"id","label","extensions":[],"input":"file|directory"}]}`。保存済みのカスタムプロファイルは `id = "custom:<name>"` で表示されます。

### `detect --input <path>`

フォーマットを調べます。JSON 出力：`{"engine","content_root","label","profile"}`。組み込みアダプターが先、次に保存済みカスタムプロファイル。

### `analyze --input <path> [--src ja|en]`

未知の入力の偵察レポート：`builtin_detect`、`saved_profile_detect`、ファイルなら `details`（サイズ、`binary`、`encoding`（Shift-JIS/GBK 検出を含む）、行数、JSON 形状、`sample_head`）、ディレクトリなら拡張子ヒストグラム + 覗き見したサンプルファイル。最後に `next_steps` の提案が続きます。

### `profile`

| サブコマンド | 役割 |
|------------|------|
| `new --output <path> [--name <name>]` | ドキュメント付きルールテンプレートを書き出す |
| `test --profile <path|name> --input <path> [--src] [--limit 10] [--roundtrip]` | 試し抽出；一致したユニットを報告（`--roundtrip` ではメモリ内 writeback も実行） |
| `save --profile <path> [--force]` | プロファイルをユーザープロファイルディレクトリに保存 |
| `list` | 保存済みプロファイルを JSON で表示 |

### `init --input <path> --src ja|en --dst zh [--engine <id>] [--profile <path|name>] [--workspace <dir>]`

ワークスペースを登録 / 開く（`attx.db`、`workspace.json` を作成）。`--engine` は組み込みを強制；`--profile` はカスタムを設定（`<workspace>/profile.toml` にコピー、engine は `custom:<name>`）。デフォルトのワークスペース：`<dir>/.attx` または `<parent>/.attx-<stem>`。

### `extract --workspace <dir> [--no-knowledge]`

アダプター → DB へのテキストユニット。JSON 出力：`{"extracted","skipped_by_knowledge","rules_applied","status"}`。`--no-knowledge` は学習済みルールをすべて無視します（学習前の挙動）。

### `translate --workspace <dir> [--limit N] [--dry-run] [--retry-passthrough]`

保留ユニットに対する LLM。増分保存されます。JSON 出力：`{"pending_before","translated","pending_after","passthrough","dry_run","skipped_note"}`。stderr に `batch i/n` の進捗が表示されます。`--dry-run` はモデルを呼ばずにバッチ計画を出力；`--retry-passthrough` は最初に passthrough ユニットを再キューします。

### `writeback --workspace <dir> [--dry-run] [--no-learn]`

翻訳出力をレンダリング。JSON 出力：`{"files","units_applied","dry_run","paths":[]}` — 自動の post-writeback 経験サマリーが実行された場合は `learned` も。`--dry-run` は計画のみ；`--no-learn` はこの実行のサマリーをスキップします。

### `run --input <path> [--engine] [--profile] [--src] [--dst] [--workspace] [--limit] [--no-translate] [--no-writeback] [--glossary] [--no-glossary]`

init → extract →（用語集が有効または強制なら）→ translate → writeback、1 つの JSON レポート。`--glossary` は `[glossary].enabled` が false でも用語集を構築；`--no-glossary` はこの実行では禁止します。用語集の構築失敗は非致命的です（`glossary.error` に報告されます）。

### `status --workspace <dir>`

`{"engine","game_path","source_lang","target_lang","total","translated","pending","passthrough","domains":{…}}`。`passthrough > 0` → `translate --retry-passthrough` を検討。

### `review --workspace <dir>`

機械的な翻訳後スキャン（LLM なし）。JSON：`total` / `translated` / `pending` / `passthrough`、`glossary`（`glossary check` と同じ形）、バケット `residual_source` / `identical` / `control_loss` / `namebox_mismatch`（`count` + `sample` 最大 40）。`attx run` は translate の後にこれを付ける。

### `preserve`

| サブコマンド | 役割 |
|------------|------|
| `list --workspace <dir>` | 組み込み + ワークスペース正規表現（JSON） |
| `add --workspace --pattern <re> [--info]` | ワークスペース規則を追加。ヒットは `[CTRL_n]` になる |
| `remove --workspace --pattern <re>` | 完全一致の pattern で削除 |

組み込みは常に適用：RMMZ 制御コード、`{ident}`、`%s`/`%d`。エンジン `renpy` は `[ident]` も保護。ワークスペースファイル：`preserve.toml`。

### JSONL データ交換

| コマンド | 役割 |
|---------|------|
| `translate-jsonl --input --output [--src] [--dst] [--limit]` | ワークスペースなし：`{id,text,context?,role?,item_type?}` が入力、`translation`+`translation_lines` が追加されて出力 |
| `export-jsonl --workspace --output [--filter pending\|all\|translated\|passthrough]` | ワークスペース → JSONL |
| `import-jsonl --workspace --input` | JSONL → ワークスペース（`id` で照合） |

### `learn`

| サブコマンド | 役割 |
|------------|------|
| `summarize --workspace <dir> [--llm]` | 実行の証拠を skip フィールドエントリに変換（`scan` はエイリアス）；`--llm` はそれらの提案をレビュー（費用がかかります） |
| `note --text "…" [--name <id>] [--workspace <dir> \| --format <id>]` | 文体ノートを書く。デフォルト `topic=prompt` は次の translate のシステムプロンプトに注入される |
| `pending` | 承認待ちのエントリ（証拠付き、JSON） |
| `review --approve 1,3 [--reject 2] [--approve-all]` | 1 ベースのインデックスで承認 / 却下 |
| `list [--format <id>] [--workspace <dir>]` | アクティブなエントリ（JSON）。`--workspace` はこの作品の `experience.toml` を出す |
| `defaults --format <id>` | フォーマットの組み込みベースラインを出力（TOML） |
| `forget --field <name> [--format <id>]` | フィールド名で skip/extract エントリを削除 |
| `forget --name <id> [--workspace <dir>]` | `learn note` で書いたノートを削除 |

### `glossary`

| サブコマンド | 役割 |
|------------|------|
| `build --workspace <dir> [--min-occurrences N] [--dry-run]` | LLM で固有名詞を `glossary.toml` に抽出；常に最初にドライランを |
| `list [--all]` | 用語を JSON で表示（`--all` はモデルに却下されたものを含む） |
| `add --src <term> --dst <translation> [--info <desc>]` | 用語を 1 つ追加 / 上書き |
| `remove --src <term>` | 用語を 1 つ削除 |
| `import --file <json>` / `export --file <json>` | `[{src,dst,info}]` または `{src: dst}` |
| `check` | 翻訳が実際には使わなかった用語（違反、件数付き） |
