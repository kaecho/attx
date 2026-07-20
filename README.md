# attx

**English** | [中文](README.zh-CN.md)

**Agent Translation Toolkit eXtensible** — a pure-Rust, engine-agnostic game text translation framework for AI agents and humans.

```
extract (engine adapter) → translate (LLM core) → writeback (engine adapter)
```

| Adapter | Target |
|---------|--------|
| `rmmz` | RPG Maker MV / MZ (`data/*.json` dialogue, System, base DB) |
| `jsonl` | Generic JSONL text packs (any engine via external extract/write scripts) |

Inspired by the generalisation goal in [att-mz#11](https://github.com/yexi-by/att-mz/issues/11).

---

## Install

### Release binary

Download from [Releases](https://github.com/emptysuns/attx/releases) (tags `v*`).

### From source

```bash
git clone https://github.com/emptysuns/attx.git
cd attx
cargo build --release
./target/release/attx --help
# optional:
cargo install --path .
```

---

## Configure the LLM

```bash
cp setting.example.toml setting.toml
```

Edit `setting.toml`:

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"
base_url = "https://your-provider.example/v1"
api_key = "YOUR_API_KEY"
model = "your-model-name"
timeout = 600

[translation]
worker_count = 8
rpm = 60
retry_count = 3
retry_delay = 2
batch_chars = 2500
max_context_items = 6
```

Notes:

- `provider_type` must be `openai` (OpenAI-compatible Chat Completions).
- `base_url` usually ends with `/v1`.
- `setting.toml` is gitignored — never commit API keys.

Check connectivity:

```bash
attx doctor --ping
```

---

## How to translate a game (RPG Maker MV/MZ)

### 1. Detect the engine

```bash
attx detect --game /path/to/game
# → {"engine":"rmmz","content_root":"...","label":"RPG Maker MV/MZ"}
```

### 2. Create a workspace

```bash
attx init --game /path/to/game --src ja --dst zh
# default workspace: /path/to/game/.attx
# or:
attx init --game /path/to/game --src ja --dst zh --workspace /tmp/my-game-ws
```

- `--src`: source language (`ja` or `en`)
- `--dst`: target language (prompted as Simplified Chinese today)

### 3. Extract text

```bash
attx extract --workspace /path/to/game/.attx
attx status --workspace /path/to/game/.attx
```

Extracts dialogue (event codes 101/401/102/405), `System.json` terms, and base DB fields (`Actors`, `Items`, …).

### 4. Translate with the LLM

```bash
# full pending set
attx translate --workspace /path/to/game/.attx

# small trial batch
attx translate --workspace /path/to/game/.attx --limit 20

# plan only
attx translate --workspace /path/to/game/.attx --dry-run
```

Identical source hashes are cached in the workspace DB; re-runs skip already translated units.

### 5. Write translations back into the game

```bash
# preview files that would change
attx writeback --workspace /path/to/game/.attx --dry-run

# apply (creates one-shot *.attxbak beside each rewritten file)
attx writeback --workspace /path/to/game/.attx
```

Then launch the game and playtest.

### One-shot pipeline

```bash
attx run --game /path/to/game --src ja --dst zh
# options:
#   --limit N
#   --no-translate
#   --no-writeback
#   --workspace /custom/ws
```

### Manual / offline path (JSONL)

Useful for review, external tools, or non-RM engines:

```bash
attx export-jsonl --workspace .attx --output pending.jsonl --filter pending
# edit or translate pending.jsonl elsewhere, then:
attx import-jsonl --workspace .attx --input translated.jsonl
attx writeback --workspace .attx
```

Standalone JSONL (no game tree):

```bash
# line format: {"id":"scene1:55","text":"…","context":"op","role":"Hero"}
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh
```

---

## CLI reference

| Command | Role |
|---------|------|
| `doctor [--ping]` | Config check / optional LLM ping |
| `detect --game` | Engine probe |
| `init --game` | Create workspace + SQLite |
| `extract` | Adapter → text units |
| `translate` | LLM over pending units |
| `writeback` | Adapter applies translations |
| `run` | init + extract + translate + writeback |
| `status` | Counts |
| `translate-jsonl` | Pure text pipe |
| `export-jsonl` / `import-jsonl` | Interchange |

Global: `--config /path/to/setting.toml` (default: `./setting.toml` or `$ATTX_HOME/setting.toml`).

---

## Extend with a new engine

Implement `EngineAdapter` in `src/adapter/` and register it in `all_adapters()`:

```rust
pub trait EngineAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn detect(&self, game_path: &Path) -> Option<DetectHit>;
    fn extract(&self, content_root: &Path, source_lang: &str) -> Result<Vec<TextUnit>>;
    fn writeback(
        &self,
        content_root: &Path,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<BTreeMap<String, String>>; // relative path → file body
}
```

No core pipeline changes required. For engines not yet implemented, extract to JSONL externally, run `translate-jsonl`, then write back with your own script.

---

## Project layout

```
src/
  main.rs          CLI
  model.rs         TextUnit / Translation / control placeholders
  config.rs        setting.toml
  store.rs         SQLite workspace
  llm.rs           OpenAI-compatible chat + batching
  quality.rs       line / control checks
  pipeline.rs      orchestration
  adapter/
    mod.rs         trait + registry
    rmmz.rs        RPG Maker MV/MZ
    jsonl.rs       generic pack
```

---

## Not included (yet)

- Plugin JS AST / note-tag agent workflows (see att-mz)
- RGSS Marshal / encrypted archives
- First-class Unity / Ren'Py / Godot adapters

Use the `jsonl` path until those adapters land. PRs welcome.

---

## License

MIT
