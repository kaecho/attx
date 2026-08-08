# Install

## Release binary

Download the archive for your OS from [GitHub Releases](https://github.com/emptysuns/attx/releases) (tags `v*`). Put `attx` / `attx.exe` on your `PATH`.

## From source

```bash
git clone https://github.com/emptysuns/attx.git
cd attx
cargo build --release
./target/release/attx --help
cargo install --path .   # optional
```

## LLM config — two paths

### A. Agent Q&A (recommended)

Install the Skill, then ask the agent to set up attx. It walks endpoint → key → model → languages and writes `setting.toml` without echoing the key. See [Agents](agents.md).

### B. Manual

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

[translation]
worker_count = 8
rpm = 60
batch_chars = 2500
```

```bash
attx doctor --ping
```

Search order: `--config` → `./setting.toml` → `$ATTX_HOME/setting.toml`. Never commit API keys.
