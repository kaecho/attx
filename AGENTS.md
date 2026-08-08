# Repository Guidelines

Guidelines for AI assistants working in the `attx` repository.

## Project Overview

**attx** (Agent Translation Toolkit eXtensible) is a pure-Rust, single-binary, format-agnostic AI translation framework for coding agents and humans. It extracts text units from games/ebooks/documents/subtitles/localization files, translates them with any OpenAI-compatible LLM, and writes them back. Progress is cached in a SQLite workspace so interrupted runs resume for free. Format support is modeled after AiNiee's reader/writer plugin set, reimplemented as Rust adapters.

Core pipeline: `extract (format adapter) → translate (LLM core) → writeback (format adapter)`.

The agent-facing protocol is authoritative: `skills/attx/SKILL.md` + `skills/attx/references/*.md` define stages, hard stops, and the CLI contract. README.md and `docs/` are the human-facing mirror.

## Architecture & Data Flow

```
CLI (src/main.rs, clap derive)
  → config::load (setting.toml)
  → pipeline::init_workspace → Store::open → <workspace>/attx.db (+ workspace.json)
  → pipeline::extract
       adapter::detect / detect_or_force / resolve_adapter → FormatAdapter::extract
       → Vec<TextUnit> → optional knowledge::apply filter → store.replace_units
  → [optional] glossary::build → glossary.toml
  → pipeline::translate
       store.pending_units → Translator::translate_units_with_sink (worker threads)
       → on_batch: store.save_translation (incremental, resumable)
       → LLM hard failure → passthrough (original text kept, flagged)
  → pipeline::writeback
       adapter.writeback → OutputFile bytes → sibling copy or in-place + *.attxbak
       → learn::summarize (if auto_summarize) → experience entries
```

Key facts:

- **Central type**: `model::TextUnit` (`id`, `engine`, `domain`, `location`, `item_type` (`long_text|array|short_text`), `role`, `original_lines`, `source_line_paths`, `context`, `payload`). `TextUnit::compute_id` = sha256(engine\0location\0lines) truncated to 24 hex chars.
- **Adapters are pure I/O** — no network. Trait `FormatAdapter: Send + Sync` (`src/adapter/mod.rs:59`) with `id/label/extensions/input_kind/detect/extract/writeback`. Registry `all_adapters()` (`mod.rs:97`) order = detect priority; JSON sniffers go most-specific first.
- **Workspace**: directory input → `<dir>/.attx/`; file input → sibling `.attx-<stem>/` (`pipeline.rs:837-850`). Contains `attx.db` (tables `meta`, `units`, `translations`; WAL mode), `workspace.json`, optional `glossary.toml`, `experience.toml`, `profile.toml`.
- **No async runtime**: `std::thread::scope` workers + `reqwest::blocking` + a `Mutex`-based `RateLimiter` (`llm.rs`). `Store` is `!Sync`, so SQLite saves are drained on the caller thread via `mpsc`.
- **Output convention**: document/localization formats write a translated sibling `<stem>.<dst>.<ext>`; only `rmmz` (and custom profiles with `overwrite = true`) write in place, with a one-time `*.attxbak` backup created by pipeline.
- **Config search** (`config.rs:219`): `--config` → `$ATTX_HOME/setting.toml` → `./setting.toml` (missing config is OK for non-LLM commands).

## Key Directories

| Path | Purpose |
|------|---------|
| `src/` | Core crate: `main.rs` (CLI), `pipeline.rs` (orchestration), `llm.rs` (LLM client/workers), `store.rs` (SQLite), `model.rs` (TextUnit), `config.rs`, `glossary.rs`, `knowledge.rs` + `learn.rs` (experience layer), `profile.rs` (custom formats), `quality.rs` (output QA), `textio.rs` (encoding) |
| `src/adapter/` | One `XxxAdapter` per format (rmmz, rmmz_plugins, epub, docx, xlsx, subtitle, ass, csv, po, renpy, plaintext, jsonkv, jsonl, xmllite); `mod.rs` = trait + registry |
| `src/defaults/` | Embedded experience defaults (`rmmz.toml`, via `include_str!` in `knowledge.rs:45`) |
| `skills/attx/` | **Agent protocol**: `SKILL.md` (stages -1..8, hard stops, Q&A wizard) + `references/` (CLI contract, agent usage, failure recovery, custom format discovery, JSONL workflow, feedback iteration) |
| `docs/{en,zh,ja}/` | Human docs, MkDocs Material + static-i18n → `site/` (generated, gitignored) |
| `profiles/examples/` | Sample custom-format profiles (json-messages, ini-lang, kirikiri-kag) |
| `.github/workflows/` | `ci.yml` (build+test+smoke, ubuntu+windows), `release.yml` (4-target GitHub Releases), `docs.yml` (Pages) |

## Development Commands

```bash
cargo build --release            # CI does release builds; debug works too
cargo test                       # full unit test suite (inline #[cfg(test)] only)
cargo test <module>::            # filter, e.g. cargo test glossary::
cargo test <test_name>           # e.g. cargo test epub_roundtrip
cargo run --release -- doctor    # config check (smoke test)
cargo run --release -- --help
cargo clippy                     # not enforced in CI, but keep it clean

# docs (Python)
pip install -r requirements-docs.txt   # mkdocs-material>=9.5, mkdocs-static-i18n>=1.2
mkdocs build --strict                   # output → site/
```

CI (`.github/workflows/ci.yml`): rust stable via `dtolnay/rust-toolchain`, matrix ubuntu-latest + windows-latest, steps `cargo build --release` → `cargo test` → smoke `doctor` + `--help`. No clippy/rustfmt/coverage gates. Releases: tag `v*` → `cargo build --release --target` for `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `x86_64-apple-darwin`, packaged with README, LICENSE, setting.example.toml, skills/, profiles/. Not published to crates.io.

## Code Conventions & Common Patterns

- **Errors**: `anyhow::Result` everywhere; no custom error enum. `bail!`, `.with_context(|| format!(...))`, `anyhow::anyhow!`. CLI prints `error: {err:#}` to stderr and exits 1.
- **Serde**: `#[derive(Serialize, Deserialize)]` with `#[serde(default)]` on optional sections (backward-compatible settings); enums `rename_all = "snake_case"` or `"lowercase"`; custom profiles use `#[serde(deny_unknown_fields)]`. CLI reports are `#[derive(Serialize)]` structs printed as JSON on stdout.
- **Naming**: snake_case (Rust + TOML keys); test names descriptive snake_case (`pending_skip_is_inert`); `///` doc comments on public items, `//!` module docs on the larger modules.
- **Concurrency**: never introduce async — blocking reqwest + `std::thread` workers; rate limit via `RateLimiter` (`rpm`, `rpm = 0` disables); retries via `retry_count`/`retry_delay`.
- **Resilience**: LLM batch failure → split to singles; single failure → `passthrough` (original text stored, `passthrough = true`), never a dead run. Malformed knowledge/glossary files degrade to empty fallback with an `eprintln!`; learn failures never fail writeback.
- **Quality**: `quality.rs` runs after every batch — `sanitize_lines` soft-fixes (line-count pad/truncate, empty fallback), `check_unit` hard-rejects (empty output, Array line-count mismatch, control-code loss).
- **Encoding**: text adapters go through `textio::read_text` (UTF-8 → UTF-16 BOM → `chardetng` → `encoding_rs`); output is always UTF-8. Zip/JSON formats use plain UTF-8 paths.
- **Mark deliberate ceilings** with `ponytail:` comments (e.g. `glossary.rs:46` `EXTRACT_MAX_BATCHES = 40`). There are currently no TODO/FIXME markers in core src.
- **Config keys** (`setting.toml`): `[llm] default_client` + `[[llm.clients]]` (`name`, `provider_type`, `base_url`, `api_key`, `model`, `timeout`); `[translation] worker_count=8, rpm=60, retry_count=3, retry_delay=2, batch_chars=2500, max_context_items=6`; `[glossary] enabled=false, method=llm|stats, min_occurrences=10, max_terms=200, inject_limit=30`; `[learn] auto_summarize=true, llm_review=false`. Env: `ATTX_HOME` (config + `profiles/` + `knowledge/`), fallback `dirs::config_dir()/attx/`.

## Important Files

| File | Why it matters |
|------|----------------|
| `src/main.rs` | CLI surface: every subcommand, global `--config`/`--client`, dispatch (`Cli`/`Commands`/`ProfileCommands`/`LearnCommands`/`GlossaryCommands`) |
| `src/pipeline.rs` | End-to-end orchestration: `init_workspace`, `extract`, `translate`, `writeback`, `run`, `status`, JSONL paths, `analyze`, `formats`, `doctor` |
| `src/adapter/mod.rs` | `FormatAdapter` trait + `all_adapters()` registry (detect order) + `output_sibling` naming |
| `src/llm.rs` | OpenAI-compatible client (`{base_url}/chat/completions`), worker pool, RateLimiter, passthrough |
| `src/store.rs` | `attx.db` schema (`meta`/`units`/`translations`), pending-unit query, WAL |
| `src/model.rs` | `TextUnit`/`Translation`/`WorkspaceMeta`, `compute_id`, `needs_translation` heuristics |
| `src/config.rs` | `Settings` structs, `resolve_config_path`, `example_toml()` (source of `setting.example.toml`) |
| `skills/attx/SKILL.md` | The authoritative agent workflow: stages, hard stops (401 → stop; `learn review --approve-all` forbidden; rmmz writeback needs permission), Q&A wizard rules, "never hand-edit inputs/attx.db/tool source", API keys only in `setting.toml` |
| `Cargo.toml` | Crate `attx` 0.7.0, edition 2024, release profile `lto=true, codegen-units=1, strip=true`, no feature flags, no MSRV declared |

## Runtime/Tooling Preferences

- **Toolchain**: stable Rust, edition 2024 (needs a recent stable; no rust-toolchain.toml, no MSRV). No nightly features.
- **No dev-dependencies**; tests use std + `temp_dir()` fixtures. Do not add test frameworks.
- **Dependency style**: major-version pins (`anyhow = "1"`, `clap = { version = "4", features = ["derive"] }`, `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls", "blocking"] }`, `rusqlite = { version = "0.37", features = ["bundled"] }`, `zip` with `deflate` only). HTTP is deliberately rustls + blocking.
- **No formatter/linter config** (no rustfmt.toml, no clippy.toml); rely on rustfmt defaults, occasional `#[allow(clippy::…)]`.
- **gitignore**: `setting.toml` (API keys — never commit), `.attx/`, `.attx-*/`, `*.db`, `*.attxbak`, `*.epub`, `/site/`, `/target`.
- **Docs**: MkDocs Material, folder-structure i18n (`docs/{en,zh,ja}/`, en default); build must pass `--strict`.

## Testing & QA

- **All tests are inline `#[cfg(test)] mod tests` at file bottom** in `src/` — no `tests/` directory, no integration-test crate. ~90 tests across 20 files.
- **Coverage focus**: adapters dominate — extract → fake `Translation` map → writeback roundtrips with fixtures built in-test (zip writers for epub/docx/xlsx, inline string samples for ass/po/renpy/subtitle/jsonkv). No committed binary fixtures.
- **Core logic tests**: `glossary.rs` (18), `knowledge.rs` (~24, incl. embedded defaults parse), `learn.rs` (10), `config.rs` (3), `model.rs` (3), `llm.rs` (2).
- **Untested modules** (regressions show only via CLI smoke): `pipeline.rs`, `store.rs`, `profile.rs`, `quality.rs`, `textio.rs`, `main.rs`, `adapter/jsonl.rs`. When changing these, run the CLI smoke (`doctor`, `--help`, and ideally a real `run` on a small fixture) manually.
- **Temp dirs**: `std::env::temp_dir()/attx-test-{pid}-{name}` via `adapter::test_dir`; some knowledge tests set `ATTX_HOME` — use `--test-threads=1` if those collide.
- **QA expectation**: no coverage thresholds, no doctests. CI only builds, tests, and smokes. Keep new logic tested the same way (inline unit test, temp-dir fixture).
