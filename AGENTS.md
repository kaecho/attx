# Repository Guidelines

`attx` (Agent Translation Toolkit eXtensible) is a pure-Rust, single-binary translation CLI. It extracts text units from games, ebooks, documents, subtitles, localization files, or a custom TOML profile, translates them through any OpenAI-compatible Chat Completions endpoint, and writes the result back. A SQLite workspace caches every accepted translation so interrupted runs resume.

## Project Overview

- Crate `0.12.0`, edition `2024`, `rust-version = "1.89"`, MIT. One binary target, `attx` (`src/main.rs`). No library target, feature flags, or `[dev-dependencies]`.
- Core pipeline: `identify → extract → optional glossary → translate → mechanical review and bounded repair → safe writeback`.
- Built-in adapters: `rmmz` (RPG Maker MV/MZ), `epub`, `html`, `docx`, `xlsx`, `srt`, `vtt`, `ass`, `lrc`, `csv`/TSV, `po`, `renpy`, `md`, `txt`, JSON sniffers `paratranz`, `vnt`, `mtool`, `i18next`, the `jsonl` escape hatch, and the generic `auto` content sniffer. Saved custom profiles use `custom:<name>`.
- Outputs: document, subtitle, and JSON adapters write a sibling such as `book.zh.epub`. RMMZ writes in place. Custom profiles with `overwrite = true` write in place. `auto` on a directory writes `translated-<dst>/` and copies unsupported files unchanged. First replacement of an existing destination creates `{path}.attxbak`.
- Configuration is `setting.toml`; API keys live only there. Non-LLM commands work with a missing config, but translation, glossary, and LLM review paths call `require_llm`.
- Distribution is GitHub Releases (`v*` tag or `workflow_dispatch`) for Linux x86_64, Windows x86_64, macOS arm64, and macOS x86_64. Archives include the binary, READMEs, CHANGELOG, LICENSE, `setting.example.toml`, `mkdocs.yml`, `requirements-docs.txt`, `skills/`, `profiles/`, and `docs/`. Do not `cargo publish`.

## Architecture & Data Flow

```text
src/main.rs (clap)
  → config::load
  → detect_any: built-in adapters → saved custom profiles → AutoAdapter
  → init_workspace: .attx.lock, attx.db, workspace.json, profile snapshot
  → extract: FormatAdapter::extract → knowledge::apply → Store::replace_units
  → [glossary::build]
  → translate: pending + blocking units → Translator
       → std::thread::scope workers → sink saves on the caller thread
       → retry / split / passthrough
  → review::blocking_unit_ids
  → writeback: normalize/reflow → adapter render → stage → backup → commit
       → Store::replace_published
  → learn::summarize (failure never fails writeback)
```

### CLI Surface

```text
doctor [--ping] [--json]                          formats
detect --input <file-or-dir>                      analyze --input <file-or-dir> [--src ja]
profile new|infer|test|save|list
learn summarize (alias: scan) | note | pending | review | list | defaults | forget
learn layout add|list|remove|test                  slot/label rules
glossary build|list|add|remove|import|export|check
preserve list|add|remove
init --input <file-or-dir> [--engine <id>|--profile <path-or-name>] [--workspace <dir>] [--infer]
extract --workspace <dir> [--no-knowledge]
translate --workspace <dir> [--limit N] [--dry-run] [--retry-passthrough]
repair --workspace <dir> [--limit N] [--dry-run]
writeback --workspace <dir> [--dry-run] [--no-learn] [--allow-partial]
run --input <file-or-dir> [--engine <id>|--profile <path-or-name>] [--src ja] [--dst zh]
status --workspace <dir>                          review --workspace <dir>
translate-jsonl --input <file> --output <file>
export-jsonl --workspace <dir> --output <file> [--filter pending|all|translated|passthrough]
import-jsonl --workspace <dir> --input <file>
```

- Global options: `--config <path>` and `--client <name>`. Defaults: `--src ja`, `--dst zh`; `profile test --limit 10`; `export-jsonl --filter pending`; `learn note --topic prompt`.
- `run` performs init, extract, optional glossary, translate, and writeback. It aborts when extraction yields zero units. `--no-translate` skips translate and writeback; `--no-writeback` keeps extraction and translation.
- Glossary on `run` only when translation is enabled, `--no-glossary` is absent, and `--glossary` or `[glossary].enabled` is set. A fatal LLM error aborts; other glossary errors are recorded in the JSON report.
- Without `--engine`/`--profile`, `run` can infer a declarative profile when detection falls to `auto` (unless `--no-infer`). `--infer` runs that inference for generic `auto`, including under `--no-translate`. If inference fails after `auto` matched, it falls back to `auto` with `profile_inference.status = "fallback"`; if nothing matched, the inference error is fatal.
- `detect` has no `--engine`; force an engine on `init` or `run`. `--game` is an input alias.

### Workspace and State

- Directory input: `<input>/.attx/`. File input: `<parent>/.attx-<stem>/` (`pipeline::default_workspace`).
- An existing workspace is bound to its engine, `content_root`, source language, and target language. Mismatches abort. The copied `profile.toml` is SHA-verified, and the extracted unit-id set is checked again before writeback.
- `attx.db` uses WAL and tables `meta`, `units`, `translations`, and `published`. `Store` owns a `rusqlite::Connection` and is `!Sync`; all DB writes stay on the caller thread.
- `TextUnit` fields: `id`, `engine`, `domain`, `location`, `item_type` (`long_text|array|short_text`), `role`, `original_lines`, `source_line_paths`, `context`, `payload`.
- `TextUnit::compute_id` is the first 24 hex characters of SHA-256 over `engine\0location\0` plus each line and `\n`. `source_hash` is the full SHA-256 of the lines. A cached translation is valid only when its `source_hash` matches.
- `replace_units` deletes and reinserts the current unit set, then drops translations and published rows whose unit vanished or changed source hash. `pending_units` joins on both id and matching hash, so a stale cache becomes pending automatically.
- `fileio::WorkspaceLock` holds `.attx.lock` for the process lifetime. `fileio::StagedFile` uses a held `cap-std` directory handle, `same-file` identity checks, Unix `0600` temporary files, symlink/non-regular refusal, and atomic rename. A directory-wide writeback is not a filesystem transaction: every output is staged first, then committed sequentially.

### Translation Core

- `llm.rs` is deliberately synchronous: blocking `reqwest`, `std::thread::scope`, and a global `RateLimiter` shared by first attempts, retries, and split requests. Never introduce an async runtime or a tokio API.
- Requests go to `{base_url}/chat/completions`. `extra` is merged into the body last except `messages`. Effective timeout is `client.timeout.max(30)`.
- Batching splits on `batch_chars`, `max_context_items`, and scene changes (`engine`, `context`, file). Prompt ids are positional within each batch (`1..n`). Unknown or duplicate ids are dropped; malformed rows are skipped without invalidating other rows.
- `LlmFailure` separates fatal errors from retryable transport errors. `408`, `429`, and `5xx` retry. Other HTTP errors, including `401`, `403`, and `404`, are fatal and stop queued work. Exhausted units become `passthrough` translations that retain the source text and set the flag.
- Accepted output must pass preserve-token multiplicity/order checks and `quality::check_translation`. `quality::normalize_lines` applies safe fixes before hard rejection. `review::blocking_unit_ids` treats passthrough, structural failure, and visible kana residue as blocking; glossary and speaker consistency are advisory. Speaker-label structure is checked only when an approved `kind = "layout"` experience rule matches the source slot.
- `review` reports `residual_source`, `kana_edge`, `kana_mixed`, `kana_untranslated`, `identical`, `control_loss`, `residual_latin`, and `glossary.violations`; samples are capped at 40. Only the blocking conditions above gate writeback.
- `translate` and `run` perform `repair_rounds + 1` passes. `repair` performs `max(repair_rounds, 1)` targeted passes. Repair is bounded; it is not an infinite retry loop.

### Adapters and Output

- `FormatAdapter` (`src/adapter/mod.rs`) exposes `id`, `label`, `extensions`, `input_kind`, `detect`, `extract`, and `writeback`. Adapters are pure I/O: no HTTP, no SQLite, no workspace policy.
- `all_adapters()` order is detect priority: `rmmz`, `epub`, `html`, `docx`, `xlsx`, `srt`, `vtt`, `ass`, `lrc`, `csv`, `po`, `renpy`, `md`, `txt`, `paratranz`, `vnt`, `mtool`, `i18next`, `jsonl`, `auto`. `adapter::detect` skips `auto`; `pipeline::detect_any` runs built-ins, then saved profiles, then `AutoAdapter`. `--engine` uses `detect_or_force` and claims the forced engine even when detection soft-fails.
- `output_sibling` produces `<stem>.<target>.<ext>`. `jsonl` writes a file sibling or `<dir>/translated.jsonl`. `rmmz` writes live `data/` and `js/plugins.js`.
- RMMZ extraction prefers `data_origin/` when it contains `System.json`; writeback always targets `content_root/data/`. Domains are `dialogue`, `scroll`, `namebox`, `choices`, `system`, `base`, and `plugins`. Event commands are never inserted or deleted. `401`/`405` text is refitted into the existing slot count using the active `kind = "layout"` rule; the built-in rule sets `44` half-width cells. Overflow is retained and reported.
- RMMZ plugin `.js` sources are never rewritten. Only plugin parameters in `js/plugins.js` are updated, and a plugin extract error is skipped rather than fatal.
- `auto` sniffs JSON, XML, YAML, INI, TOML, and prose with byte spans. Directory mode copies unsupported files, preserves source encoding and permissions, and excludes `.attx*`, `.git`, `node_modules`, `translated-*`, backups, editor backups, and symlinks. Report copy-only and excluded coverage honestly.
- Custom profiles are declarative and `deny_unknown_fields`: `name`, `label`, `extensions`, `detect_regex`, `min_units`, `overwrite`, `skip_lines`, `notes`, and `rules` (`line_regex`, `json_keys`, `json_paths`). `profile infer` is limited to three attempts, forces `overwrite = false`, and must pass extraction plus a lossless source roundtrip.

### Writeback Gate

- `writeback` drops passthrough and blocking units from the write set. If any unit would be skipped and `--allow-partial` is absent, status is `blocked`, no files are written, and a non-dry-run process exits `2`.
- `--allow-partial` writes valid units and leaves unresolved units at their originals. It is an explicit completeness decision, not a workaround for a fatal error.
- Before rendering, the source unit set is re-extracted and compared with `source_snapshot`. Adapter render errors and backup failures stop publication. `--dry-run` renders in memory and reports planned paths without writing.

### Configuration

- Lookup order: `--config <path>` → `$ATTX_HOME/setting.toml` only if that file exists → `./setting.toml`, returned even when missing.
- A missing config produces empty `llm.clients` and section defaults, so `detect`, `extract`, `status`, and similar commands still work. `require_llm` is the paid-path guard.
- `ATTX_HOME` is also the first choice for `profiles/` and `knowledge/`; the fallback is `dirs::config_dir()/attx/{profiles,knowledge}`. The platform config directory is never searched for `setting.toml`.
- Only `[llm]` is mandatory when a config file exists. `[translation]`, `[glossary]`, and `[learn]` are `#[serde(default)]`.

## Key Directories

| Path | Purpose |
|---|---|
| `src/` | CLI, pipeline, LLM, store, model, config, glossary, knowledge, learn, profile, quality, review, layout, textio, fileio |
| `src/adapter/` | One adapter or helper per format; `mod.rs` holds the trait and registry |
| `src/defaults/` | Embedded built-in experience (`rmmz.toml`, field and layout entries, included by `knowledge.rs`) |
| `skills/attx/` | Authoritative agent protocol: `SKILL.md` plus `references/` |
| `docs/{en,zh,ja}/` | Human documentation, 16 pages per locale; MkDocs Material with folder i18n |
| `profiles/examples/` | Template profiles, not auto-loaded |
| `.github/workflows/` | `ci.yml`, `release.yml`, `docs.yml` |

No `scripts/`, repository `*.sh`, Makefile, or justfile exists. Local checks are the documented cargo and MkDocs commands.

## Development Commands

```bash
cargo build --release --locked
cargo test --locked -- --test-threads=1
cargo test glossary::                 # filter by module path
cargo test epub_roundtrip             # filter by test name
cargo clippy --all-targets            # local hygiene, not CI-gated
cargo run --release -- doctor --json
cargo run --release -- --help
cargo install --path . --locked

# Documentation
python -m venv .venv
.venv/bin/pip install -r requirements-docs.txt
.venv/bin/mkdocs build --strict       # output: site/ (gitignored)
```

CI uses `cargo build --release --locked`, then `cargo test --locked -- --test-threads=1`, then smokes `doctor` and `--help`. Rustfmt and Clippy are not gated. `cargo test` runs 230 inline unit tests; there is no integration crate and no doctest target.

## Code Conventions & Common Patterns

### Formatting, Naming, Errors

- Default rustfmt style. No `rustfmt.toml` or `clippy.toml`.
- Rust `snake_case`; clap converts command and flag names to kebab-case. Test names are behavior sentences such as `pending_skip_is_inert` and `*_roundtrip`.
- Errors use `anyhow::Result`, `bail!`, and `.with_context(...)`. There is no custom error enum. `main` prints `error: {err:#}` to stderr and exits `1`.
- Nearly every command prints serde JSON to stdout. `doctor` is human text unless `--json`; `detect` prints compact JSON; `learn defaults` prints raw TOML. `translate`, `repair`, `writeback`, `run`, and `translate-jsonl` exit `2` when their status is non-`ok` and the run is not a dry run; `status` and `review` still exit `0`.
- `serde` derives use `#[serde(default)]` on optional sections, `rename_all = "snake_case"` or `"lowercase"` on enums, and `deny_unknown_fields` on custom profiles. Declaration order is serialization order. `serde_json` is built with `preserve_order`, which is load-bearing for JSON writeback fidelity.
- No `chrono`; timestamps are Unix seconds. Regexes are compiled once with `LazyLock`.
- Comments explain invariants and safety asymmetry. Deliberate ceilings are marked with `ponytail:` comments. There are no `TODO` or `FIXME` markers in `src/`.

### Concurrency, State, and Dependencies

- Never add async. Use blocking `reqwest`, `std::thread::scope`, `mpsc`, atomics, and the shared `RateLimiter`.
- There is no dependency-injection framework. Functions take `&Settings`, `&Store`, or `&dyn FormatAdapter`; adapters are stateless trait objects.
- SQLite is the single source of truth for extracted units and translations. Do not add a parallel in-memory cache or bypass `Store`/`fileio` for workspace mutations.
- Dependencies are crates.io major pins only, with no git or path dependencies. `reqwest` uses `rustls-tls` and blocking, not native TLS. `rusqlite` uses `bundled`. `zip` enables only `deflate`. `tokio` is only a transitive dependency of `reqwest`; do not expose it as an API.
- Adding an adapter means a new `src/adapter/<name>.rs`, an `impl FormatAdapter`, registration in `all_adapters()` at the correct priority, and an inline extract → fake translation → writeback roundtrip test. Keep `registry_ids_unique` green. JSON sniffers go most-specific first.

### Safety Invariants

- `knowledge::apply` is a pure filter that only removes units. An `Extract` entry cannot invent a unit the adapter never produced; a machine-literal `Extract` is vetoed. Layers are builtin (`rmmz` only), then `$ATTX_HOME/knowledge/<engine>.toml`, then `<workspace>/experience.toml`. `--no-knowledge` restores pre-learning extraction exactly.
- Learned `Skip` proposals stay `Pending` until `attx learn review` approves them. Never use `--approve-all` as an agent. Translation-style notes go through `learn note`; proper nouns go through the glossary.
- Layout rules are `kind = "layout"` experience entries. `label` is matched at the first visible byte after `control` tokens and whitespace, `width` is half-width cells (`0` inherits an approved width for the domain), and `leading_controls` selects label or body for a leading control. Built-in RMMZ rules live in `src/defaults/rmmz.toml`. Pending rules do not apply. Add game-specific rules with `learn layout add --workspace`, validate with `learn layout test`, and keep them out of global scope unless the user accepts the wider effect.
- The glossary is off by default. `run` builds one only when translation is enabled, `--no-glossary` is absent, and `[glossary].enabled` or `--glossary` is set. It splits extraction into 3500-character batches, capped at 40. `glossary check` is advisory.
- Preserve rules mask literals as `[CTRL_n]` before translation. Non-reorderable literals require exact multiplicity and order after unmasking.
- `export-jsonl` writes `id` = `location`, so its own output roundtrips. Imported JSONL must match an existing unit by `location`, then by `id`; source text must match exactly. Unknown ids, duplicates, lost preserve tokens, and structural failures reject the import.
- Never hand-edit inputs, `attx.db`, `experience.toml`, or tool source to fake progress. Never put API keys in chat, prompts, command arguments, logs, JSONL, commits, or reports.

### Agent Protocol and Hard Stops

- Authoritative order: live `attx --help`, stdout JSON, and exit code; then `skills/attx/SKILL.md` plus `references/cli-command-contract.md`; then `README.md` and `docs/`. Human docs are a mirror and can lag.
- `doctor` returns JSON `status: "ok"` even when the LLM is unconfigured or the ping fails. Inspect `llm.configured` and the `ping` string, not the top-level status.
- Default execution is `attx --config <cfg> run --input <input> --src ja --dst zh`. `run` owns detection, extraction, optional glossary, translation, bounded repair, and writeback. Do not recreate an unbounded translate/review loop in the agent layer.
- Normal translation of a named input authorizes normal writeback, including RMMZ in-place data and an explicitly selected `overwrite = true` profile. No second writeback-permission prompt is required. Backups and source-integrity checks still apply.
- Stop on `401`/`403` or another permanent 4xx, invalid endpoint/model/config, backup failure, binary/encrypted/lossy/ambiguous input, or a blocked writeback without explicit user acceptance of partial output.
- Profile inference is bounded to three attempts and must not execute model-generated code. Repair is bounded by `repair_rounds`. Do not restart a run just to reset either budget.
- `ok` means the requested operation finished, not that the translation is perfect. `needs_attention` and exit `2` require reporting pending, passthrough, residual, and skipped counts. A dry run is not a generated output.

## Important Files

| File | Why it matters |
|---|---|
| `src/main.rs` | Entire CLI surface, global `--config`/`--client`, JSON dispatch, exit-status mapping |
| `src/pipeline.rs` | `detect_any`, `init_workspace`, `extract`, `translate`, `repair`, `writeback`, `run`, `status`, `review`, JSONL, `analyze`, `profile_infer`, `formats`, `doctor` |
| `src/model.rs` | `TextUnit`, `Translation`, `WorkspaceMeta`, `JsonlRecord`, id/hash computation |
| `src/store.rs` | SQLite schema, migrations, pending query, cache invalidation, passthrough and published state |
| `src/llm.rs` | OpenAI-compatible HTTP, batching, worker pool, rate limiting, retries/splitting, parsing, passthrough |
| `src/fileio.rs` | Workspace lock, staged writes, atomic commit, backups, symlink and identity checks |
| `src/adapter/mod.rs` | `FormatAdapter`, `all_adapters()`, `detect`/`detect_or_force`, `output_sibling`, JSON path helpers |
| `src/adapter/auto.rs` | Generic content sniffing, coverage report, `translated-<dst>/` output, copy-only handling |
| `src/adapter/rmmz.rs` | RMMZ detect/extract/writeback, `data_origin`, anchor hashes, slot fitting, domains |
| `src/adapter/rmmz_plugins.rs` | Plugin parameter extraction and `js/plugins.js` writeback; never rewrites plugin sources |
| `src/profile.rs` | `FormatProfile`, `custom:<name>`, saved-profile discovery, declarative rules |
| `src/config.rs` | `Settings`, config search, defaults, `example_toml()`, `require_llm` |
| `src/knowledge.rs` | Experience layers and entry kinds (`field`, `layout`, `note`), `apply`, built-in `rmmz.toml` |
| `src/learn.rs` | Evidence thresholds, pending proposals, notes, post-writeback summarize |
| `src/glossary.rs` | Glossary build, term selection, batch injection, `check` |
| `src/preserve.rs` | Built-in and workspace regexes, `[CTRL_n]` masking, multiplicity checks |
| `src/quality.rs` | `normalize_lines` safe fixes and `check_translation` hard rejection |
| `src/layout.rs` | Data-driven label/control/width rules, slot fitting, reflow primitives |
| `src/review.rs` | Mechanical scan, blocking set, sample-capped report |
| `src/textio.rs` | Encoding detection, UTF-8 output contract |
| `skills/attx/SKILL.md` | Authoritative agent workflow, authorization boundaries, hard stops |
| `skills/attx/references/cli-command-contract.md` | Field-level CLI and JSON contract |
| `docs/{en,zh,ja}/` | Human docs, 16 pages per locale; `docs/*/agents.md` is the short agent guide |
| `profiles/examples/` | `ini-lang.toml`, `json-messages.toml`, and `kirikiri-kag.toml` (`overwrite = true`) |
| `Cargo.toml` | Version, edition, MSRV, dependencies, release profile |
| `.github/workflows/` | CI matrix, release targets and packaging, GitHub Pages docs |

## Runtime/Tooling Preferences

- Stable Rust, minimum `1.89`, edition 2024. No `rust-toolchain.toml`, no rustfmt/clippy config, no `.cargo/config.toml`, no MSRV CI job. CI and release float `dtolnay/rust-toolchain@stable`.
- Cargo is the package manager and build tool. `Cargo.lock` is committed and matches `0.12.0`. There is no `[workspace]`, no `[features]`, no build script, and no `[lib]`.
- Release profile: `lto = true`, `codegen-units = 1`, `strip = true`.
- Native binary only. No Node, Bun, npm, or Python runtime dependency for the tool itself.
- Documentation uses Python 3.12, `mkdocs-material>=9.5`, and `mkdocs-static-i18n>=1.2`. `mkdocs build --strict` must pass.
- `ATTX_HOME` selects config (only when `setting.toml` exists there), saved profiles, and global knowledge. Platform config fallback is `dirs::config_dir()/attx/{profiles,knowledge}`.
- Never commit `setting.toml`, `.attx/`, `.attx-*/`, `*.db`, `*.attxbak`, source artifacts (`*.epub`, `*.zh.txt`, `*.zh.json`), `/target`, or `/site/`.

Configuration defaults:

| Section | Defaults |
|---|---|
| `[llm.clients]` | `provider_type = "openai"`, `timeout = 600`; `temperature` omitted (0.3 translate, 0.0 JSON), `stream = false` |
| `[translation]` | `worker_count = 8`, `rpm = 60`, `retry_count = 3`, `retry_delay = 2`, `batch_chars = 2500`, `max_context_items = 6`, `repair_rounds = 2`, `context_chars = 1200` |
| `[glossary]` | `enabled = false`, `min_occurrences = 10`, `max_terms = 200`, `inject_limit = 30` |
| `[learn]` | `auto_summarize = true`, `llm_review = false` |

`rpm = 0` disables rate limiting. `repair_rounds = 0` disables extra translate/run passes.

## Testing & QA

- 230 `#[test]` functions across 30 of 32 source files, all in inline `#[cfg(test)] mod tests` at file bottoms. No `tests/` crate, no benches, no doctests, no coverage thresholds, no `[dev-dependencies]`.
- Untested: `src/main.rs` and `src/adapter/jsonl.rs`. Thin coverage: `src/store.rs` (1), `src/config.rs` and `src/model.rs` (2 each). `src/pipeline.rs` has 5 tests. CLI dispatch, SQLite behavior, and quality gates can regress while `cargo test` stays green, so run a real CLI smoke when touching those paths.
- Test style: `use super::*`, behavior-sentence names, plain `assert!`/`assert_eq!`, std plus production crates, no network, no subprocesses.
- Shared fixture `adapter::test_dir(name)` returns `temp_dir()/attx-test-{pid}-{name}`, creates it, and never cleans it. Most tests leave directories behind. `auto.rs` uses a counter and a `Drop` guard; `rmmz_plugins` and a few others remove before create.
- Zip fixtures are built in-test with `zip::ZipWriter`. EPUB keeps `mimetype` stored and first; other entries use deflate. DOCX and XLSX fixtures follow the same pattern.
- `knowledge::tests::file_missing_or_broken_degrades_to_empty` mutates `ATTX_HOME` process-wide with `unsafe { set_var(...) }`. Run `cargo test -- --test-threads=1` when touching `ATTX_HOME`, pid-keyed temp dirs, or that test. CI always uses `--test-threads=1`.
- The standard roundtrip recipe: write a fixture, call `adapter.extract`, build a `BTreeMap<String, Translation>` keyed by unit id with `source_hash`, call `writeback`, then assert output bytes and path. Examples: `ass.rs:191`, `subtitle.rs:285`, `epub.rs:441`, `docx.rs:250`, `xlsx.rs:225`, `po.rs:232`, `csv.rs:251`, `jsonkv.rs:500`, `plaintext.rs:190`, `auto.rs:960`.
- New logic in untested modules belongs in an inline test in the same file. Do not add a test framework, a `tests/` crate, or network calls for unit tests unless asked.
- CI runs on `ubuntu-latest` and `windows-latest` with `fail-fast: false`: release build, single-threaded tests, then `doctor` and `--help`. Release verification also runs the tests, checks `attx --version`, runs `doctor --json`, and builds docs with `mkdocs build --strict`. Clippy, rustfmt, audit, and coverage are not gated.
