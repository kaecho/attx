# attx

**Agent Translation Toolkit eXtensible** — pure-Rust universal game text translation framework.

```
extract (engine adapter) → translate (LLM core) → writeback (engine adapter)
```

Core is engine-agnostic. Engines plug in as adapters. Ship with:

| Adapter | Target |
|---------|--------|
| `rmmz` | RPG Maker MV / MZ (`data/*.json` dialogue, System, base DB) |
| `jsonl` | Generic JSONL text packs (any engine via external extract/write scripts) |

Issue inspiration: [att-mz#11](https://github.com/yexi-by/att-mz/issues/11) — generalise beyond RM-only tooling.

## Install

### Release binary

Download from [Releases](https://github.com/emptysuns/attx/releases) (tag `v*`).

### From source

```bash
cargo install --path .
# or
cargo build --release
./target/release/attx --help
```

## Configure LLM

```bash
cp setting.example.toml setting.toml
# edit base_url / api_key / model
```

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"
base_url = "https://api.example.com/v1"
api_key = "YOUR_API_KEY"
model = "example-model"
timeout = 600
```

## Quick start (RPG Maker MV/MZ)

```bash
# detect engine
attx detect --game /path/to/game

# workspace under /path/to/game/.attx by default
attx init --game /path/to/game --src ja --dst zh
attx extract --workspace /path/to/game/.attx
attx status --workspace /path/to/game/.attx

# needs setting.toml
attx translate --workspace /path/to/game/.attx
attx writeback --workspace /path/to/game/.attx

# one-shot
attx run --game /path/to/game --src ja --dst zh
```

Writeback writes `data/*.json` and keeps a one-shot `*.attxbak` backup beside each file.

## Generic JSONL (any engine)

```bash
# external tool extracts:
# {"id":"scene1:55","text":"…","context":"00_op","role":"Hero"}
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh

# or via workspace export/import
attx export-jsonl --workspace .attx --output pending.jsonl --filter pending
attx import-jsonl --workspace .attx --input translated.jsonl
attx writeback --workspace .attx
```

## CLI

| Command | Role |
|---------|------|
| `doctor` | config / optional LLM ping |
| `detect` | engine probe |
| `init` | create workspace + SQLite |
| `extract` | adapter → units |
| `translate` | LLM pending units |
| `writeback` | adapter apply translations |
| `run` | init+extract+translate+writeback |
| `status` | counts |
| `translate-jsonl` | pure text pipe (no game tree) |
| `export-jsonl` / `import-jsonl` | interchange |

## Extend with a new engine

Implement `EngineAdapter` in `src/adapter/`:

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
    ) -> Result<BTreeMap<String, String>>; // relpath → file body
}
```

Register in `adapter::all_adapters()`. No core changes required.

## Layout

```
src/
  main.rs          CLI
  model.rs         TextUnit / Translation / placeholders
  config.rs        setting.toml
  store.rs         SQLite workspace
  llm.rs           OpenAI-compatible chat + batching
  quality.rs       line/control checks
  pipeline.rs      orchestration
  adapter/
    mod.rs         trait + registry
    rmmz.rs        MV/MZ
    jsonl.rs       generic pack
```

## What this is not (yet)

- Plugin JS AST / note-tag rule agent workflow (att-mz has those)
- RGSS Marshal / rgssad archives
- Unity / Ren'Py / Godot first-class adapters

Use `jsonl` + external extractors until those adapters land. PRs welcome.

## License

MIT
