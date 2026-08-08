# インストール

## リリースバイナリ

[GitHub Releases](https://github.com/emptysuns/attx/releases)（タグ `v*`）からお使いの OS 向けアーカイブをダウンロードします。対応ターゲット：Linux x86_64、Windows x86_64、macOS x86_64 + aarch64。`attx` / `attx.exe` を `PATH` に通してください。

## ソースからビルド

```bash
git clone https://github.com/emptysuns/attx.git
cd attx
cargo build --release
./target/release/attx --help
cargo install --path .   # 任意
```

最近の安定版 Rust（edition 2024）が必要です。nightly 機能は不要、MSRV の固定もありません。

## LLM 設定 — 2 つの方法

### A. エージェントの Q&A（推奨）

Skill をインストールし、エージェントに attx のセットアップを依頼します。endpoint → key → model → 言語 の順に進み、キーをエコーせずに `setting.toml` を書き込みます。[Agents](agents.md)（エージェント）を参照。

### B. 手動

```bash
cp setting.example.toml setting.toml
```

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"          # OpenAI 互換 Chat Completions
base_url = "https://api.example.com/v1"
api_key = "YOUR_API_KEY"
model = "your-model"
timeout = 600                     # 秒、リクエストごと

[translation]
worker_count = 8       # 並列 HTTP バッチ数
rpm = 60               # 1 分あたりのグローバルレート制限（0 = 無制限）
retry_count = 3
retry_delay = 2
batch_chars = 2500     # バッチあたりの最大ソース文字数
max_context_items = 6  # バッチあたりの最大ユニット数
```

そして確認：

```bash
attx doctor --ping
```

`doctor` は設定をチェックし、組み込みアダプターと保存済みプロファイルを一覧表示します；`--ping` は LLM に小さなリクエストを 1 つ送信します。機械可読形式：`attx doctor --json`。

### 設定の検索順序

`--config <path>` → `$ATTX_HOME/setting.toml` → `./setting.toml`。

- `ATTX_HOME` には保存済みプロファイルと学習済み経験も置かれます：`$ATTX_HOME/profiles/`、`$ATTX_HOME/knowledge/`。
- `ATTX_HOME` がない場合、プラットフォームの設定ディレクトリが使われます（Linux では `~/.config/attx/`）。
- `setting.toml` は gitignore されています — **API キーをコミットしてはいけません**。
- `--client <name>` は、1 回の呼び出しについて非デフォルトの `[[llm.clients]]` エントリに切り替えます。

## インストールの確認

```bash
attx formats                 # 組み込みアダプターの一覧（JSON）
attx detect --input <file>   # 入力を受け持つアダプターはどれか
attx --help
```
