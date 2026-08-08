# インストール

## リリースバイナリ

[GitHub Releases](https://github.com/emptysuns/attx/releases)（`v*` タグ）から OS 用アーカイブを取得し、`attx` / `attx.exe` を `PATH` に置きます。

## ソースから

```bash
git clone https://github.com/emptysuns/attx.git
cd attx
cargo build --release
./target/release/attx --help
cargo install --path .   # 任意
```

## LLM 設定

```bash
cp setting.example.toml setting.toml
```

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"
base_url = "https://api.example.com/v1"
api_key = "YOUR_API_KEY"
model = "your-model"
timeout = 600
```

```bash
attx doctor --ping
```

探索順：`--config` → `./setting.toml` → `$ATTX_HOME/setting.toml`。
