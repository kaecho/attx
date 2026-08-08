# Install

## Release binary

Download the archive for your OS from [GitHub Releases](https://github.com/emptysuns/attx/releases) (tags `v*`).

Unpack and put `attx` / `attx.exe` on your `PATH`.

## From source

```bash
git clone https://github.com/emptysuns/attx.git
cd attx
cargo build --release
./target/release/attx --help
# optional
cargo install --path .
```

Requires a recent stable Rust toolchain.

## LLM config

```bash
cp setting.example.toml setting.toml
```

Edit:

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

[translation]
worker_count = 8
rpm = 60
batch_chars = 2500
```

`setting.toml` is gitignored. Check with:

```bash
attx doctor --ping
```

Config search order: `--config` → `./setting.toml` → `$ATTX_HOME/setting.toml`.
