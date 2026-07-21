# attx

**English** | [中文](README.zh-CN.md)

**Agent Translation Toolkit eXtensible** — a pure-Rust, single-binary, format-agnostic AI translation framework for agents and humans.

```
extract (format adapter) → translate (LLM core) → writeback (format adapter)
```

Translate games, ebooks, documents, subtitles, and localization files with any OpenAI-compatible LLM. Progress is cached in a SQLite workspace, so interrupted runs resume for free. Format support is modeled after [AiNiee](https://github.com/NEKOparapa/AiNiee)'s reader/writer plugin set, reimplemented as Rust adapters.

## Supported formats

| id | Input | Notes | Output |
|----|-------|-------|--------|
| `rmmz` | directory | RPG Maker MV/MZ: `data/*.json` events/system/DB + plugin params in `js/plugins.js` (plugin *source* never modified) | in-place + `*.attxbak` |
| `epub` | `.epub` | E-books / light novels: paragraph-level, ruby readings (`<rt>`) stripped from source text, images & layout preserved, `dc:language` updated | `<name>.<dst>.epub` |
| `docx` | `.docx` | Word documents: paragraph-level over `w:t` runs | `<name>.<dst>.docx` |
| `txt` | `.txt` | Plain-text novels, one unit per line (UTF-8; `iconv` legacy encodings first) | `<name>.<dst>.txt` |
| `md` | `.md` | Markdown: code fences skipped, heading/list/quote prefixes preserved | `<name>.<dst>.md` |
| `srt` / `vtt` | file | Subtitles: timing lines & headers verbatim, cue text translated | translated copy |
| `lrc` | `.lrc` | Lyrics: timestamps kept, `[ti:…]` meta tags skipped | translated copy |
| `po` | `.po` `.pot` | Gettext: fills `msgstr`; plural entries & header pass through | translated copy |
| `renpy` | `.rpy` | Ren'Py `translate` blocks: dialogue + `old`/`new` strings | translated copy |
| `mtool` | `.json` | MTool `ManualTransFile.json` (content-sniffed) | translated copy |
| `paratranz` | `.json` | Paratranz export; only empty `translation` fields are filled | translated copy |
| `vnt` | `.json` | VNTextPatch export (`name`/`message`) | translated copy |
| `i18next` | `.json` | Nested JSON with string leaves | translated copy |
| `jsonl` | file/dir | Universal escape hatch: any engine via external extract/write scripts | `translated.jsonl` |

`attx formats` prints this list as JSON. The four `.json` flavors are distinguished by content sniffing; force with `--engine <id>` when ambiguous.

Not yet supported (adapter contributions welcome, see [Contributing](#contributing)): Translator++ projects, XLSX, PDF, ASS subtitles, non-UTF-8 input.

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

## Use with an AI agent (Skill)

attx ships an **execution Skill** — a protocol the agent follows instead of improvising:

```text
skills/attx/SKILL.md           # stages, hard stops, Q&A config wizard
skills/attx/references/        # CLI contract, agent usage, recovery, JSONL, feedback
```

### Install the skill

**Claude Code** (recommended):

```bash
# personal, all sessions:
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
# or project-scoped:
mkdir -p .claude/skills && cp -a skills/attx .claude/skills/
```

The agent then discovers `attx` in its skill list and routes translation requests to it automatically (or invoke explicitly with `/attx`).

**Any other agent** (Cursor / Codex / OpenCode / …): keep the repo checkout accessible and say:

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
```

### Q&A-driven configuration

You do not need to hand-edit config to get started. If `setting.toml` is missing or invalid, the skill instructs the agent to run an **interactive wizard**: it asks for your API endpoint (OpenAI / DeepSeek / custom relay), API key (written straight to `setting.toml`, never echoed back), model name, language pair, and concurrency — then verifies with `attx doctor --ping`. Just tell the agent:

```text
Help me set up attx, then translate ./novel.epub from Japanese to Simplified Chinese.
```

### Example agent prompt

```text
Use the attx toolkit at <attx-dir>, following skills/attx/SKILL.md, to translate
<input file or game dir> from Japanese into Simplified Chinese.

Rules:
1. Only operate through the attx CLI; never hand-edit inputs, attx.db, or tool source.
2. If the LLM is not configured, run the Q&A config wizard first; never print my API key.
3. doctor --ping → detect → init → extract → status → translate --limit 20 → full translate.
4. Ask before in-place writeback (RPG Maker); document formats produce translated copies.
5. Report counts and next step after each stage.
```

---

## Configure the LLM (manual path)

```bash
cp setting.example.toml setting.toml
```

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"          # OpenAI-compatible Chat Completions
base_url = "https://your-provider.example/v1"
api_key = "YOUR_API_KEY"
model = "your-model-name"
timeout = 600

[translation]
worker_count = 8       # parallel HTTP batches
rpm = 60
retry_count = 3
retry_delay = 2
batch_chars = 2500     # max source chars per batch
max_context_items = 6  # max units per batch
```

`setting.toml` is gitignored — never commit API keys. Verify with `attx doctor --ping`.

---

## Usage

### Translate a document / ebook / subtitle file

```bash
attx run --input "novel.epub" --src ja --dst zh
# → writes novel.zh.epub next to the input; the original is never touched
```

Step-by-step (recommended for large inputs — trial 20 units first):

```bash
attx detect  --input book.epub
attx init    --input book.epub --src ja --dst zh      # workspace: .attx-book/
attx extract --workspace .attx-book
attx status  --workspace .attx-book
attx translate --workspace .attx-book --limit 20      # trial
attx translate --workspace .attx-book                 # full; re-run to resume
attx writeback --workspace .attx-book                 # → book.zh.epub
```

Real-world validation: a full 4,171-paragraph light novel EPUB (10.9 MB with illustrations) translated ja→zh-Hans in one run — 100% coverage, EPUB structure/images intact, TOC and `dc:title`/`dc:language` localized.

### Translate an RPG Maker MV/MZ game

```bash
attx run --input /path/to/game --src ja --dst zh --no-writeback
attx writeback --workspace /path/to/game/.attx --dry-run   # preview
attx writeback --workspace /path/to/game/.attx             # in-place + *.attxbak
```

### Manual / offline review path (JSONL)

```bash
attx export-jsonl --workspace .attx-book --output pending.jsonl --filter pending
# review/edit translation_lines externally, then:
attx import-jsonl --workspace .attx-book --input pending.jsonl
attx writeback    --workspace .attx-book
```

Standalone, no workspace:

```bash
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh
```

### CLI reference

| Command | Role |
|---------|------|
| `doctor [--ping]` | Config check / LLM ping |
| `formats` | Supported adapters as JSON |
| `detect --input <path>` | Format probe (`--game` alias kept) |
| `init --input <path> --src --dst` | Create workspace + SQLite |
| `extract --workspace` | Adapter → text units |
| `translate --workspace [--limit] [--dry-run]` | LLM over pending units, incremental saves |
| `writeback --workspace [--dry-run]` | Render translated output |
| `run --input …` | init + extract + translate + writeback |
| `status --workspace` | Counts |
| `translate-jsonl` / `export-jsonl` / `import-jsonl` | Interchange |

Global: `--config /path/to/setting.toml` (default `./setting.toml` or `$ATTX_HOME/setting.toml`).

---

## Contributing

PRs welcome — new format adapters especially. The codebase is deliberately small and boring; keep it that way.

### Architecture

```
src/
  main.rs          CLI (clap)
  model.rs         TextUnit / Translation / control-code masking / language probes
  config.rs        setting.toml
  store.rs         SQLite workspace (units, translations, hash cache)
  llm.rs           OpenAI-compatible chat, batching, parallel workers, prompts per profile
  quality.rs       line-count / control-code sanity checks
  pipeline.rs      orchestration (no format knowledge, no HTTP in adapters)
  adapter/
    mod.rs         FormatAdapter trait + registry + shared helpers
    xmllite.rs     lossless mini-XML tree (epub/docx)
    epub.rs docx.rs plaintext.rs subtitle.rs po.rs renpy.rs jsonkv.rs
    rmmz.rs rmmz_plugins.rs jsonl.rs
```

Layering rule: **adapters do parsing/serialization only** — batching, LLM calls, caching, retries, and disk writes live in the pipeline. An adapter never talks to the network.

### Add a new format adapter

1. Create `src/adapter/myformat.rs` implementing the trait:

```rust
pub trait FormatAdapter: Send + Sync {
    fn id(&self) -> &'static str;               // stable, used in --engine & DB
    fn label(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str] { &[] } // empty → directory input
    fn detect(&self, input: &Path) -> Option<DetectHit>;    // default: by extension
    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>>;
    fn writeback(&self, input: &Path, target_lang: &str,
                 units: &[TextUnit], translations: &BTreeMap<String, Translation>)
                 -> Result<Vec<OutputFile>>;    // absolute paths + bytes
}
```

2. Register it in `all_adapters()` in `src/adapter/mod.rs` (order = detect priority; content-sniffing `.json` adapters go most-specific-first).
3. Rules of thumb:
   - Emit units only for text that `needs_translation(text, source_lang)`.
   - `location` must be a stable, zero-padded address (`c00042`) — it is the writeback anchor and the batch sort key.
   - `context` groups consecutive units into the same LLM batch (chapter/file/section).
   - Untranslated units must survive writeback unchanged; document formats write a translated **sibling copy**, never modify the input.
   - If the model must preserve inline tokens, mask them (`model::mask_controls`) or extend the system prompt in `llm.rs`.
4. Add a round-trip unit test in the same file (`build tiny sample → extract → fake translations → writeback → assert`). See `epub.rs` tests for the pattern.
5. `cargo fmt && cargo clippy && cargo test` must pass; update the format tables in both READMEs and `skills/attx/SKILL.md`.

### PR checklist

- [ ] Round-trip test for every new adapter (fixtures built in-test, no binary blobs in git)
- [ ] No new dependencies unless a hand-rolled version would be meaningfully worse
- [ ] `cargo fmt` / `cargo clippy` clean, CI green (Linux + Windows)
- [ ] No API keys, sample game data, or copyrighted content in the diff
- [ ] READMEs (EN + zh-CN) and SKILL.md format tables updated

### Roadmap (grab one)

- Translator++ (.trans) and XLSX adapters
- ASS subtitle adapter
- Shift-JIS / GBK auto-detection (encoding_rs)
- Glossary / terminology pinning across batches
- PDF via external tooling (as AiNiee does with BabelDOC)

---

## License

MIT
