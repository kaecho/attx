# attx

**English** | [中文](README.zh-CN.md) | [Docs](https://emptysuns.github.io/attx/)

**Agent Translation Toolkit eXtensible** — a pure-Rust, single-binary, format-agnostic AI translation framework for agents and humans.

```
extract (format adapter) → translate (LLM core) → writeback (format adapter)
```

Translate ebooks, documents, subtitles, localization files, and games with any OpenAI-compatible LLM. Progress is cached in a SQLite workspace, so interrupted runs resume for free. Format support is modeled after [AiNiee](https://github.com/NEKOparapa/AiNiee)'s reader/writer plugin set, reimplemented as Rust adapters.

- **19 built-in adapters** — EPUB, HTML, DOCX, XLSX, TXT/MD, SRT/VTT/ASS/LRC, CSV, PO, Ren'Py, RPG Maker MV/MZ, MTool/Paratranz/VNTextPatch/i18next JSON, plus a generic JSONL interchange format.
- **Custom format profiles** — teach attx any unknown text/JSON format with a small TOML file (`line_regex` / `json_keys` / `json_paths` rules).
- **Resumable by design** — every run is checkpointed in `attx.db`; stop anytime, continue anytime. Failed units become visible *passthrough* placeholders instead of killing the run.
- **Self-improving** — successful runs leave extraction experience behind (`skip`/`extract` field judgements), reviewed by you, never applied silently to delete text.
- **Glossary** — one agreed translation per proper noun across a whole work, injected per batch.

---

## Quick start for agents (recommended)

attx is designed so a coding agent can **read the Skill, ask you a few questions, write `setting.toml`, and run the pipeline** — you should not need to hand-edit config first.

### 1. Install the binary

- **Release:** [Releases](https://github.com/emptysuns/attx/releases) (tags `v*`)
- **From source:**

```bash
git clone https://github.com/emptysuns/attx.git
cd attx
cargo build --release
./target/release/attx --help
# optional:
cargo install --path .
```

### 2. Install the Skill (so the agent knows the protocol)

```text
skills/attx/SKILL.md           # stages, hard stops, Q&A config wizard
skills/attx/references/        # CLI contract, agent usage, custom-format discovery, recovery, JSONL, feedback
```

**Claude Code:**

```bash
# personal, all sessions:
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
# or project-scoped:
mkdir -p .claude/skills && cp -a skills/attx .claude/skills/
```

**Any other agent** (Cursor / Codex / OpenCode / …): keep the checkout and say:

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
```

**Why a Skill instead of an MCP server?** attx is a local CLI with JSON on stdout — that is already the native tool surface for coding agents. A Skill is plain markdown any agent can follow; MCP would only wrap the same CLI behind a long-lived process.

### 3. One prompt — agent configures via Q&A, then translates

If `setting.toml` is missing or `attx doctor` fails, the Skill **requires** an interactive wizard. The agent asks, one item at a time:

1. API endpoint (OpenAI / DeepSeek / custom OpenAI-compatible `base_url`)
2. API key → written only to `setting.toml`, **never echoed** back into chat
3. Model name
4. Language pair (`src` / `dst`)
5. Optional concurrency / glossary

Then it runs `attx doctor --ping` and continues the pipeline.

Copy-paste:

```text
Use the attx toolkit at <attx-dir>, following skills/attx/SKILL.md.

Help me set up attx if needed (Q&A wizard: endpoint, key, model, languages),
then translate <input path> from Japanese into Simplified Chinese.

Rules:
1. Only operate through the attx CLI; never hand-edit inputs, attx.db, or tool source.
2. If the LLM is not configured, run the Q&A config wizard first; never print my API key.
3. doctor --ping → detect → init → extract → status → translate --limit 20 → full translate.
4. Prefer translated copies for files; ask before any in-place overwrite.
5. Report counts and next step after each stage.
```

Shorter form also works:

```text
Help me set up attx, then translate ./novel.epub from Japanese to Simplified Chinese.
```

---

## Quick start (manual)

```bash
cp setting.example.toml setting.toml   # fill base_url / api_key / model
attx doctor --ping                     # verify config + LLM connectivity
attx run --input novel.epub --src ja --dst zh
# → writes novel.zh.epub next to the input; the original is never touched
```

For large inputs, use the step-by-step pipeline and trial with a small `--limit` first (see [Usage](#usage)).

---

## Configure the LLM

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"          # OpenAI-compatible Chat Completions
base_url = "https://your-provider.example/v1"
api_key = "YOUR_API_KEY"
model = "your-model-name"
timeout = 600                     # seconds

[translation]
worker_count = 8       # parallel HTTP batches
rpm = 60               # global request rate limit per minute (0 = unlimited)
retry_count = 3
retry_delay = 2        # seconds between retries
batch_chars = 2500     # max source chars per batch
max_context_items = 6  # max units per batch

[glossary]
enabled = false        # build during `attx run` (costs extra LLM calls)
method = "llm"         # llm | stats

[learn]
auto_summarize = true  # capture experience after writeback (free)
llm_review = false     # also ask the model to check proposals (costs money)
```

`setting.toml` is gitignored — never commit API keys. Verify with `attx doctor --ping`.

Config search order: `--config` → `$ATTX_HOME/setting.toml` → `./setting.toml`. `--client <name>` switches the LLM client for one invocation.

---

## Usage

### One-shot

```bash
attx run --input "novel.epub" --src ja --dst zh
# → novel.zh.epub next to the input
```

`run` = `init` → `extract` → (optional glossary) → `translate` → `writeback`, reporting each stage as JSON. Add `--limit 20` for a trial, `--no-writeback` to inspect before writing, `--glossary`/`--no-glossary` to override the config.

### Step-by-step (large inputs — trial 20 units first)

```bash
attx detect  --input book.epub
attx init    --input book.epub --src ja --dst zh      # workspace: .attx-book/
attx extract --workspace .attx-book
attx status  --workspace .attx-book
attx translate --workspace .attx-book --limit 20      # trial
attx translate --workspace .attx-book                 # full; re-run to resume
attx writeback --workspace .attx-book --dry-run       # preview planned files
attx writeback --workspace .attx-book                 # → book.zh.epub
```

Workspace layout: a directory input uses `<dir>/.attx`; a file input uses `<parent>/.attx-<stem>` — containing `attx.db` (units + translations + meta), `workspace.json`, and optionally `glossary.toml`, `experience.toml`, `profile.toml`.

Most file formats write a **translated copy** (`*.<dst>.*`) and leave the source untouched. The `rmmz` game adapter writes **in-place** with one-time `*.attxbak` backups — always `writeback --dry-run` first.

Real-world validation: a full 4,171-paragraph light novel EPUB (10.9 MB with illustrations) translated ja→zh-Hans in one run — 100% coverage, EPUB structure/images intact, TOC and `dc:title`/`dc:language` localized.

### When the model fails: passthrough

If a unit's translation fails repeatedly, attx stores the original text as a flagged **passthrough** placeholder so the run finishes. `attx status` reports the count; `attx translate --retry-passthrough` re-queues exactly those units.

### Manual / offline review (JSONL)

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

---

## Supported formats

| id | Input | Notes | Output |
|----|-------|-------|--------|
| `epub` | `.epub` | E-books / light novels: paragraph-level, ruby readings (`<rt>`) stripped from source text, images & layout preserved, `dc:language` updated | `<name>.<dst>.epub` |
| `html` | `.html` `.htm` `.xhtml` | Standalone HTML pages: block-level + `<title>` | translated copy |
| `docx` | `.docx` | Word documents: paragraph-level over `w:t` runs | `<name>.<dst>.docx` |
| `xlsx` | `.xlsx` `.xlsm` | Excel workbooks: shared-string table translated, all sheets consistent | translated copy |
| `txt` | `.txt` | Plain-text novels, one unit per line | `<name>.<dst>.txt` |
| `md` | `.md` `.markdown` | Markdown: code fences skipped, heading/list/quote prefixes preserved | `<name>.<dst>.md` |
| `srt` / `vtt` | file | Subtitles: timing lines & headers verbatim, cue text translated | translated copy |
| `ass` | `.ass` `.ssa` | ASS/SSA subtitles: `{\tag}` overrides & `\N` breaks preserved, Name → speaker | translated copy |
| `lrc` | `.lrc` | Lyrics: timestamps kept, `[ti:…]` meta tags skipped | translated copy |
| `csv` | `.csv` `.tsv` | Tables (RFC4180: quotes, embedded newlines); only translated records re-rendered | translated copy |
| `po` | `.po` `.pot` | Gettext: fills `msgstr`; plural entries & header pass through | translated copy |
| `renpy` | `.rpy` | Ren'Py `translate` blocks: dialogue + `old`/`new` strings | translated copy |
| `rmmz` | directory | RPG Maker MV/MZ data + plugin params in `js/plugins.js` (plugin *source* never modified) | in-place + `*.attxbak` |
| `mtool` | `.json` | MTool `ManualTransFile.json` (content-sniffed) | translated copy |
| `paratranz` | `.json` | Paratranz export; only empty `translation` fields are filled | translated copy |
| `vnt` | `.json` | VNTextPatch export (`name`/`message`) | translated copy |
| `i18next` | `.json` | Nested JSON with string leaves (≥80%) | translated copy |
| `jsonl` | file/dir | Universal escape hatch: any engine via external extract/write scripts | `translated.jsonl` |
| `custom:<name>` | file/dir | **Custom profile**: TOML rules an agent (or you) writes for any unknown text/JSON format | copy or in-place |

`attx formats` prints this list as JSON (saved custom profiles included). The four `.json` flavors are distinguished by content sniffing; force with `--engine <id>` when ambiguous.

Text inputs auto-detect their encoding (UTF-8 / UTF-16 BOM / Shift-JIS / GBK via chardetng); output is always UTF-8.

Not yet supported (adapter contributions welcome, see [Contributing](#contributing)): Translator++ projects, PDF, binary archives (use the JSONL escape hatch).

### Unknown format? Teach attx a profile

When `detect` fails, don't stop — attx ships a discovery toolchain built for agents:

```bash
attx analyze --input ./project         # recon: encoding, structure, samples, JSON shape
attx profile new --output fmt.toml     # documented rule template (line_regex / json_keys / json_paths)
attx profile test --profile fmt.toml --input ./project --roundtrip   # iterate until units look right
attx init --input ./project --profile fmt.toml --src ja --dst zh     # then extract/translate/writeback as usual
attx profile save --profile fmt.toml   # "remember this format" — detect auto-recognizes it from now on
```

A profile is a small TOML file: per-line regexes with named `text`/`role` groups, and/or JSON key/path selectors. See `profiles/examples/` (KiriKiri KAG, INI, generic JSON) and `skills/attx/references/custom-format-discovery.md` for the full agent workflow.

---

## Glossary

A model translating a long work in batches has no way to be consistent with itself: the same proper noun drifts across chapters. A glossary fixes one agreed translation per term for the whole work.

**Off by default** — building one spends extra LLM calls. Default extraction is **`llm`** (model reads source and emits terms); **`stats`** keeps the older regex-mine-then-name path (cheaper).

```bash
attx glossary build --workspace .attx --dry-run              # size the run, spend nothing
attx glossary build --workspace .attx                        # default method=llm
attx glossary build --workspace .attx --method stats         # regex mine + name
attx glossary list --workspace .attx
attx glossary add --workspace .attx --src アレイ --dst 艾蕾 --info "female given name"
attx glossary import --workspace .attx --file terms.json
attx glossary check --workspace .attx             # terms the translation ignored
```

Two methods:

```
llm   (default): source batches → model emits {src,dst,info} → vote / max_terms → inject → check
stats:           mine (regex) → min_occurrences → max_terms → name (LLM) → inject → check
```

- **`llm`**: spend tracks text batches; better recall; every `src` must be a real substring of the source (anti-hallucination).
- **`stats`**: mining/threshold are free, so the model only names frequent hits — **spend tracks term count**; lower `min_occurrences` collects more and costs more.

Statistics cannot tell a proper noun from a common word, so stats gives the model a `keep` veto; llm relies on type guidance plus the substring gate. Decided entries (including rejects) are remembered. Terms are injected per batch, only those a batch actually contains (capped by `inject_limit`).

Each entry carries a disambiguating `info` ("female given name", "place"). That is not decoration: without it the model cannot tell how a name should be addressed in context.

In `setting.toml`:

```toml
[glossary]
enabled = false        # build during `attx run`
method = "llm"         # llm | stats
min_occurrences = 10   # stats only
max_terms = 200        # cap on terms kept
inject_limit = 30      # cap on terms injected into one batch
```

An explicit `attx glossary build` ignores `enabled` — asking for it is consent. And once a `glossary.toml` exists, `translate` always injects from it: injection is nearly free, so *not* using a glossary you already built would be the surprise.

---

## Self-improving experience layer

Adapters decide what to extract with hardcoded heuristics, and those tables are sometimes wrong — a field that looks like UI text may actually be an identifier referenced verbatim by scripts. Translate it and something breaks at runtime. Until now each such fix stayed in the source, so the next project re-discovered it.

attx keeps that judgement as data, and captures it **automatically**: every successful `writeback` summarises the run into experience entries at zero API cost, because the evidence is already sitting in the workspace DB.

```bash
attx writeback --workspace .attx         # …and learn from the run, automatically
attx writeback --workspace .attx --no-learn   # opt out for one run
attx learn summarize --workspace .attx   # or trigger it by hand
attx learn summarize --workspace .attx --llm  # also ask the model (costs money)
attx learn pending                       # entries awaiting approval, with evidence
attx learn review --approve 1,3          # approve; only now do they delete anything
attx learn list                          # what is active
attx learn defaults --format rmmz        # example: built-in baseline for one format
attx learn forget --field achievename    # drop one
attx extract --no-knowledge              # escape hatch: ignore all of it
```

**The file format is open-ended on purpose.** Entries carry a `kind`, and kinds attx does not understand round-trip verbatim — so an agent can invent `kind = "voice-hint"` and attx will hand it back unchanged rather than silently dropping it. Two kinds are acted on today:

```toml
[[entry]]
kind = "field"          # a field-name extraction judgement
field = "key"
verdict = "skip"        # skip | extract
scope = "nested"        # nested | top | any
domain = "plugins"      # restrict to one unit domain; empty = any
status = "pending"      # approved | pending

[[entry]]
kind = "note"           # free-form experience; topic="prompt" reaches the model
topic = "prompt"
text = "This format loses control codes; keep every [CTRL_n] verbatim."
```

Four layers merge, later winning: built-in defaults (embedded, see `learn defaults`) → `$ATTX_HOME/knowledge/<format>.toml` → `<workspace>/experience.toml`. Within a layer, an exact field name beats a `*suffix` one and `skip` beats `extract`.

Three safeguards worth knowing:

- **Additions apply themselves; deletions wait for you.** Notes and `extract` entries take effect immediately — the worst case is a longer prompt. `skip` is the only verdict that removes text, so it is written `pending` and does nothing until `learn review --approve`. A missed translation is visible in `status`; a silently dropped line is not.
- **Learning may override a name heuristic, never the evidence of a value.** An `extract` entry is refused when the value is a number, path or script, so a bad entry cannot send switch ids or filenames to the model.
- **Entries are domain-scoped.** A rule for one domain cannot fire on another where the same field name means something else.

---

## CLI reference

| Command | Role |
|---------|------|
| `doctor [--ping] [--json]` | Config check / LLM ping |
| `formats` | Supported adapters + saved profiles as JSON |
| `detect --input <path>` | Format probe, saved profiles included (`--game` alias kept) |
| `analyze --input <path>` | Recon report for unknown inputs (encoding, structure, samples) |
| `profile new/test/save/list` | Author, iterate, and remember custom format profiles |
| `init --input <path> --src --dst [--profile]` | Create workspace + SQLite |
| `extract --workspace [--no-knowledge]` | Adapter → text units |
| `translate --workspace [--limit] [--dry-run] [--retry-passthrough]` | LLM over pending units, incremental saves |
| `writeback --workspace [--dry-run] [--no-learn]` | Render translated output; capture experience unless opted out |
| `run --input …` | init + extract + (glossary) + translate + writeback |
| `status --workspace` | Counts incl. passthrough + per-domain breakdown |
| `translate-jsonl` / `export-jsonl` / `import-jsonl` | Interchange (`--filter` incl. `passthrough`) |
| `learn summarize/pending/review/list/defaults/forget` | Self-improvement: accumulate extraction experience |
| `glossary build/list/add/remove/import/export/check` | Consistent proper-noun names across a whole work |

Global: `--config /path/to/setting.toml` (default `./setting.toml` or `$ATTX_HOME/setting.toml`); `--client <name>` picks a non-default LLM client.

Every command reports machine-readable JSON on stdout; errors go to stderr with a non-zero exit code. The exact JSON shapes are pinned in `skills/attx/references/cli-command-contract.md`.

---

## Docs

Long-form guide (EN / 中文 / 日本語): **https://emptysuns.github.io/attx/**

---

## Contributing

PRs welcome — new format adapters especially. The codebase is deliberately small and boring; keep it that way.

### Architecture

```
src/
  main.rs          CLI
  pipeline.rs      init / extract / translate / writeback / run
  adapter/         one module per format (+ custom profiles)
  llm.rs           OpenAI-compatible client, batching, masking
  store.rs         SQLite workspace
  knowledge.rs     experience layers (learn)
  glossary.rs      proper-noun glossary
  profile.rs       custom format profiles
```

### Add a new format adapter

1. Implement `FormatAdapter` in `src/adapter/<name>.rs` (`detect` / `extract` / `writeback`).
2. Register it in `src/adapter/mod.rs` (order = detect priority).
3. Add a round-trip unit test with a tiny fixture.
4. Document the id in `attx formats` output and this README.

### PR checklist

- [ ] `cargo test` green
- [ ] No API keys or sample copyrighted text in the tree
- [ ] New adapter: detect does not false-positive on other formats
- [ ] README / `formats` updated if user-visible

### Roadmap (grab one)

- More document / game / l10n adapters
- Richer custom-profile primitives
- Optional MCP wrapper for non-CLI hosts

---

## License

MIT
