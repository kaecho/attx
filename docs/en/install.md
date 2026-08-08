# Install

## Release binary

Download the archive for your OS from [GitHub Releases](https://github.com/emptysuns/attx/releases) (tags `v*`). Available targets: Linux x86_64, Windows x86_64, macOS x86_64 + aarch64. Put `attx` / `attx.exe` on your `PATH`.

## From source

```bash
git clone https://github.com/emptysuns/attx.git
cd attx
cargo build --release
./target/release/attx --help
cargo install --path .   # optional
```

Requires a recent stable Rust (edition 2024). No nightly features, no MSRV pin.

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
provider_type = "openai"          # OpenAI-compatible Chat Completions
base_url = "https://api.example.com/v1"
api_key = "YOUR_API_KEY"
model = "your-model"
timeout = 600                     # seconds, per request

[translation]
worker_count = 8       # parallel HTTP batches
rpm = 60               # global rate limit per minute (0 = unlimited)
retry_count = 3
retry_delay = 2
batch_chars = 2500     # max source chars per batch
max_context_items = 6  # max units per batch
```

Then verify:

```bash
attx doctor --ping
```

`doctor` checks config, lists the built-in adapters and saved profiles; `--ping` also sends one tiny request to the LLM. Machine-readable form: `attx doctor --json`.

### Config search order

`--config <path>` → `$ATTX_HOME/setting.toml` → `./setting.toml`.

- `ATTX_HOME` also holds your saved profiles and learned experience: `$ATTX_HOME/profiles/`, `$ATTX_HOME/knowledge/`.
- Without `ATTX_HOME`, the platform config dir is used (`~/.config/attx/` on Linux).
- `setting.toml` is gitignored — **never commit API keys**.
- `--client <name>` switches to a non-default `[[llm.clients]]` entry for one invocation.

## Verify the install

```bash
attx formats                 # list of built-in adapters as JSON
attx detect --input <file>   # which adapter claims your input
attx --help
```
