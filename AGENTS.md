# Repository Guidelines

Guidelines for AI assistants working in the `attx` repository.

## Project Overview

**attx** (Agent Translation Toolkit eXtensible) is a pure-Rust, single-binary, format-agnostic AI translation framework. It extracts text units from games, ebooks, documents, subtitles, and localization files (or a custom TOML profile), translates them with any OpenAI-compatible LLM, and writes them back. Progress is cached in a SQLite workspace so interrupted runs resume.

Crate **0.8.1**, edition **2024**, one binary (`src/main.rs`). No lib target, no feature flags, no `[dev-dependencies]`. Distribution is GitHub Releases (`tag v*` or `workflow_dispatch`); do not `cargo publish`. `Cargo.toml` does not set `publish = false`.

Core pipeline: `extract (format adapter) → translate (LLM core) → writeback (format adapter)`.

The agent-facing protocol is authoritative: `skills/attx/SKILL.md` + `skills/attx/references/*.md` define stages, hard stops, and the CLI/JSON contract. README.md and `docs/` are the human-facing mirror. If they disagree, follow the Skill + `cli-command-contract.md`, then live `attx --help`.

## Architecture & Data Flow

```
CLI (src/main.rs, clap derive)
  → config::load (setting.toml)
  → pipeline::init_workspace → Store::open → <workspace>/attx.db (+ workspace.json)
  → pipeline::extract
       detect_any: built-in adapter::detect first, else saved CustomAdapter
       adapter::detect_or_force / resolve_adapter → FormatAdapter::extract
       → Vec<TextUnit> → optional knowledge::apply (--no-knowledge skips)
       → store.replace_units
  → [optional] glossary::build → glossary.toml
  → pipeline::translate
       store.pending_units → Translator::translate_units_with_sink (std::thread::scope)
       → on_batch: store.save_translation (caller thread; incremental)
       → batch fail → split → single fail → passthrough (original kept, flagged)
  → pipeline::review (mechanical; also attached to `attx run`)
  → pipeline::writeback
       adapter.writeback → OutputFile bytes
       → first existing dest copied to `{path}.attxbak` once
       → learn::summarize if auto_summarize (failure never fails writeback)
```

Key facts:

- **Central type**: `model::TextUnit` (`id`, `engine`, `domain`, `location`, `item_type` (`long_text|array|short_text`), `role`, `original_lines`, `source_line_paths`, `context`, `payload`). `TextUnit::compute_id` = first 24 hex of `sha256(engine\0location\0 + each line + \n)`. `source_hash` is the full sha256 of lines+`\n` and is the cache key.
- **Adapters are pure I/O**, no network. Trait `FormatAdapter: Send + Sync` (`src/adapter/mod.rs`) with `id/label/extensions/input_kind/detect/extract/writeback`. Registry `all_adapters()` order = detect priority (first hit wins). `--engine` uses `detect_or_force` and still claims if detect soft-fails.
- **Workspace**: directory input → `<dir>/.attx/`; file input → sibling `.attx-<stem>/`. Contains `attx.db` (tables `meta`, `units`, `translations`; WAL), `workspace.json`, optional `glossary.toml`, `preserve.toml`, `experience.toml`, `profile.toml` (custom only).
- **No async runtime**: `std::thread::scope` workers + `reqwest::blocking` + a `Mutex`-based `RateLimiter` (`llm.rs`). `Store` is `!Sync` (owns `rusqlite::Connection`); workers send `Vec<Translation>` over `mpsc`; `on_batch` / `save_translation` run on the caller thread.
- **Output**: document / subtitle / json adapters write sibling `<stem>.<dst>.<ext>` via `adapter::output_sibling` (`book v1.epub` + `zh` → `book v1.zh.epub`). jsonl directory mode writes `<dir>/translated.jsonl`. Only `rmmz` (and custom profiles with `overwrite = true`) write in place; pipeline creates a one-time `{path}.attxbak` if the dest already exists (`Map001.json.attxbak`, not an extension replace). Ren'Py is sibling, not in-place.
- **JSONL import gotcha**: `import-jsonl` matches `JsonlRecord.id` against `unit.location`, **not** `unit.id` (`pipeline.rs`). `export-jsonl` writes `"id": location`, so attx export→import roundtrips. External JSONL using hash ids is silently skipped.
- **Config search** (`config.rs` `resolve_config_path`): `--config` → `$ATTX_HOME/setting.toml` **only if that file exists** → `./setting.toml` (returned even if missing). **`dirs::config_dir()` is not used for setting.toml.** Missing file is OK for non-LLM commands (`clients = []`). `require_llm` is what paid paths call.
- **`attx run`**: init + extract + optional glossary + translate + writeback. Glossary on `run` only if `!--no-glossary && !--no-translate && (--glossary || [glossary].enabled)`. `run` has no `--no-knowledge`. Writeback is skipped if `--no-writeback` or `--no-translate`.
- **`attx doctor`**: always exits 0 with `status: "ok"`. Ping failures are `ping: "error: …"` strings. Inspect the ping field, not the exit code. `--ping` is not part of CI.

Built-in adapter ids (detect order): `rmmz`, `epub`, `html`, `docx`, `xlsx`, `srt`, `vtt`, `ass`, `lrc`, `csv`, `po`, `renpy`, `md`, `txt`, then JSON sniffers most-specific first: `paratranz`, `vnt`, `mtool`, `i18next`, `jsonl`. `xmllite` and `rmmz_plugins` are helpers, not registry ids. Custom engines are `custom:<name>` (`profile.rs` `ENGINE_PREFIX`) and are **not** in `all_adapters()`.

RMMZ specifics that bite:

- Detect: the given path, then `www/`, `game/`, then one-level children with `data/System.json`. The first three also match a `js/` directory.
- Extract prefers `data_origin/` when `System.json` is there; **writeback always targets `content_root/data/`** (and `js/plugins.js`).
- Never insert/delete event commands; 401/405 must `fit_lines` into the original slot count. Plugin `.js` sources are never rewritten, only `plugins.js` params. Plugin extract `Err` is skipped (`eprintln`), not fatal.
- Domains: `dialogue`, `scroll`, `namebox`, `choices`, `system`, `base`, `plugins`.

Custom profile detect runs a trial `extract(input, "ja")` against `min_units`. English-only sources can fail auto-detect; force `--profile` / `--engine`.

## Key Directories

| Path | Purpose |
|---|---|
| `src/` | Core crate: CLI, pipeline, LLM, store, model, config, glossary, knowledge, learn, profile, quality, textio |
| `src/adapter/` | One adapter (or helper) per format; `mod.rs` = trait + `all_adapters()` |
| `src/defaults/` | Embedded experience (`rmmz.toml` via `include_str!` in `knowledge.rs`). Changing it changes the **binary**. |
| `skills/attx/` | **Agent protocol**: `SKILL.md` (stages -1..8, hard stops, Q&A wizard) + `references/` |
| `docs/{en,zh,ja}/` | Human docs; MkDocs Material + static-i18n → `site/` (generated, gitignored) |
| `profiles/examples/` | Sample custom-format templates. **Not auto-loaded.** User/agent: `analyze` → adapt → `profile test --roundtrip` → `profile save` or `init --profile`. |
| `.github/workflows/` | `ci.yml`, `release.yml`, `docs.yml` |

No `scripts/`, Makefile, justfile, or repo `*.sh`. No `tests/` crate.

## Development Commands

```bash
cargo build --release            # CI does release builds; debug works too
cargo test                       # full inline unit suite
cargo test glossary::            # filter by module
cargo test epub_roundtrip        # filter by test name
cargo test -- --test-threads=1   # when touching ATTX_HOME or pid-keyed temp dirs
cargo run --release -- doctor    # config check (CI smoke; no --ping)
cargo run --release -- --help
cargo clippy                     # not gated in CI; keep it clean

# docs (Python)
pip install -r requirements-docs.txt   # mkdocs-material>=9.5, mkdocs-static-i18n>=1.2
mkdocs build --strict                   # output → site/
```

CI (`.github/workflows/ci.yml`): rust **stable** via `dtolnay/rust-toolchain`, matrix `ubuntu-latest` + `windows-latest` (`fail-fast: false`). Steps: `cargo build --release` → `cargo test` → smoke `doctor` + `--help`. No clippy / rustfmt / coverage / macos / MSRV gate. Release and docs workflows do not run tests.

Releases: tag `v*` (or `workflow_dispatch`) → `cargo build --release --target` for `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `x86_64-apple-darwin`. Package includes binary + `README.md`, `README.zh-CN.md`, `LICENSE`, `setting.example.toml`, `skills/`, `profiles/`. No `cargo publish`.

## Code Conventions & Common Patterns

- **Errors**: `anyhow::Result` everywhere; no custom error enum. `bail!`, `.with_context(|| format!(...))`. CLI prints `error: {err:#}` to stderr and exits 1. Most commands print a serde JSON report on stdout; `doctor` is human unless `--json`. `learn defaults` prints raw TOML.
- **Serde**: `#[serde(default)]` on optional settings sections (0.5.0 files without `[glossary]`/`[learn]` must still parse). Enums `rename_all = "snake_case"` or `"lowercase"`. Custom profiles: `#[serde(deny_unknown_fields)]`. `serde_json` is compiled with `preserve_order` (load-bearing for JSON writeback fidelity).
- **Naming**: snake_case (Rust + TOML keys); clap default kebab-case (`translate-jsonl`, `export-jsonl`). Test names descriptive (`pending_skip_is_inert`, `*_roundtrip`). `///` on public items, `//!` on larger modules. Comments explain *why* (safety asymmetry, no-insert 401, Store `!Sync`).
- **Concurrency**: never introduce async. Blocking reqwest + `std::thread` workers. Rate limit via `RateLimiter` (`rpm`; `rpm = 0` disables). Retries via `retry_count` / `retry_delay`. Temperature 0.3 for translate, 0.0 for `ask_json`. HTTP timeout is `client.timeout.max(30)` seconds. `provider_type` is stored but unused; every client is `{base_url}/chat/completions`.
- **Batching**: `max_context_items` is max **units per HTTP batch**, not LLM context window. Prompt ids are 1-based and batch-local, not unit hashes. Unknown model ids are dropped.
- **LLM register** (`llm::profile_for_format`): `epub|txt` Literary; `srt|vtt|lrc` Subtitle; `docx|md` Document; `po|i18next|paratranz` Software; everything else Game (including `rmmz`, `ass`, `html`, `csv`, `xlsx`, `jsonl`, `renpy`).
- **Resilience**: LLM batch failure → half-split → singles → `passthrough` (`passthrough = true`); never a dead run. Empty vec after retries = skipped batch (units stay pending). `translate --retry-passthrough` → `Store::clear_passthrough`. Malformed knowledge / glossary TOML → `eprintln!` + empty. `attx run` glossary-build `Err` becomes `out.glossary.error` (non-fatal). Learn failures never fail writeback. RMMZ plugin extract `Err` is skipped.
- **Knowledge safety**: `knowledge::apply` is a pure filter that **only removes units**. `Extract` entries cannot invent units the adapter never produced. A machine-literal `Extract` increments `extract_vetoed` but **still keeps** the unit if the adapter already emitted it. Layers: builtin (0) → `$ATTX_HOME/knowledge/<engine>.toml` (1) → `<workspace>/experience.toml` (2). Skip-learning stays **pending** until `attx learn review --approve N`. Agents **must not** pass `--approve-all`. Translation style is **not** in `summarize`; agents write `topic=prompt` notes with `attx learn note --workspace` (injected on the next `translate`). Proper nouns stay in the glossary.
- **Glossary**: **off by default**. `attx run` builds one only if `[glossary].enabled` or `--glossary` (and not `--no-glossary`). Explicit `attx glossary build` ignores `enabled`. `EXTRACT_MAX_BATCHES = 40` (`glossary.rs`) caps LLM extract cost. `glossary check` is advisory. Terms must be a substring of some unit.
- **Quality**: after unmask, `sanitize_lines` soft-fixes (short_text join/fallback, array pad/truncate + empty→original, long_text trim trailing empties) then `check_unit` hard-rejects (empty output, Array length mismatch, ShortText ≠ 1 line, control-code loss if `src>0 && dst<src && dst*2 < src`). Reject drops that unit from the batch, not the run.
- **Encoding**: text adapters go through `textio::read_text` (strict UTF-8 → UTF-16 BOM → `chardetng` + `encoding_rs`); output is always UTF-8. Zip/JSON formats use UTF-8 paths. RMMZ `data/*.json` uses `fs::read_to_string` (UTF-8 assumed).
- **Adding an adapter**: new `src/adapter/<name>.rs`, impl `FormatAdapter`, register in `all_adapters()` at the right detect priority. JSON sniffers go most-specific first. Add an extract → fake `Translation` → writeback roundtrip test. Keep `registry_ids_unique` green. Do not invent a second convention beside existing adapters.
- **Deliberate ceilings**: mark with `ponytail:` comments (glossary `EXTRACT_MAX_BATCHES`, rmmz never insert/delete commands, docx mixed-style runs). No `TODO`/`FIXME` in `src/`.
- **Agent hard stops** (Skill, not the abbreviated `docs/*/agents.md` list): 401 → stop, never retry-spam keys (code still retries `retry_count` times and has no 401 special case); extract=0 with visible text → stop; rmmz / `overwrite=true` writeback needs explicit user permission (dry-run first; **code will overwrite if invoked**); `--approve-all` forbidden; never hand-edit inputs, `data/*.json`, `attx.db`, `experience.toml`, or tool source to fake progress (notes go through `learn note`); API keys only in `setting.toml`. Do not call a `--dry-run` a successful write.

## Important Files

| File | Why it matters |
|---|---|
| `src/main.rs` | Entire CLI surface: `Cli` / `Commands` / `ProfileCommands` / `LearnCommands` / `GlossaryCommands` / `PreserveCommands`; global `--config` / `--client`; JSON dispatch |
| `src/pipeline.rs` | `init_workspace`, `extract`, `translate`, `writeback`, `run`, `status`, `review`, JSONL, `analyze`, `formats`, `doctor`, workspace paths |
| `src/adapter/mod.rs` | `FormatAdapter` + `all_adapters()` + `output_sibling` |
| `src/adapter/rmmz.rs` | In-place game writeback, `data_origin` vs `data/`, `fit_lines`, domains |
| `src/adapter/rmmz_plugins.rs` | Plugin extract/writeback claimed only via `rmmz` |
| `src/adapter/jsonkv.rs` | Four `.json` sniffers (`paratranz` > `vnt` > `mtool` > `i18next`) |
| `src/adapter/jsonl.rs` | Escape hatch; file sibling or dir/`translated.jsonl`. Untested. |
| `src/llm.rs` | OpenAI-compatible `{base_url}/chat/completions`, worker pool, RateLimiter, sink, passthrough |
| `src/store.rs` | `attx.db` schema, pending query, hash invalidation, passthrough migrate |
| `src/model.rs` | `TextUnit` / `Translation` / `WorkspaceMeta` / `JsonlRecord`, `compute_id`, control-code mask |
| `src/config.rs` | `Settings` + `resolve_config_path` + `example_toml()` (source of `setting.example.toml`) |
| `src/knowledge.rs` / `src/learn.rs` | Experience layers, `apply`, post-writeback summarize |
| `src/glossary.rs` | Optional pre-translate terms + batch inject |
| `src/preserve.rs` | Regex preserve rules → `[CTRL_n]` mask (`preserve.toml` + builtins) |
| `src/review.rs` | Mechanical post-translate scan (residual source, identical, preserve loss, namebox) |
| `src/profile.rs` | `custom:<name>`, `FormatProfile` (`overwrite`, `line_regex` / `json_keys` / `json_paths`) |
| `src/quality.rs` | sanitize vs reject contract |
| `src/textio.rs` | Encoding contract for all text adapters |
| `skills/attx/SKILL.md` | Authoritative agent workflow |
| `Cargo.toml` | Crate metadata, deps, release profile |

CLI surface (kebab-case): `doctor [--ping|--json]`, `formats`, `detect`, `analyze`, `profile {new,test,save,list}`, `learn {summarize(scan),note,pending,review,list,defaults,forget}`, `glossary {build,list,add,remove,import,export,check}`, `preserve {list,add,remove}`, `init`, `extract [--no-knowledge]`, `translate [--limit|--dry-run|--retry-passthrough]`, `writeback [--dry-run|--no-learn]`, `run [--no-translate|--no-writeback|--glossary|--no-glossary]`, `status`, `review`, `translate-jsonl`, `export-jsonl`, `import-jsonl`. `--input` aliases `--game` on detect / analyze / init / run / `profile test`. Defaults: `src=ja`, `dst=zh`, export filter `pending`.

Custom profile workflow: `analyze` → `profile new` → `profile test --roundtrip` (in-memory, no disk) → `init --profile` → extract/translate/writeback → ask, then `profile save`. Save dir: `$ATTX_HOME/profiles` else `dirs::config_dir()/attx/profiles/` (Linux `~/.config/attx/profiles/`). Neither → error asking to set `$ATTX_HOME`. `kirikiri-kag.toml` is `overwrite = true`.

## Runtime/Tooling Preferences

- **Toolchain**: stable Rust, edition 2024 (needs a recent stable, rustc ≥ 1.85). No `rust-toolchain.toml`, no `rust-version` / MSRV, no rustfmt.toml / clippy.toml / `.cargo/config`. CI/release use floating `dtolnay/rust-toolchain@stable`.
- **No `[dev-dependencies]`** and no `[features]`. Tests use std + production crates (`zip`, `toml`, `serde_json`). Do not add test frameworks (`tempfile`, insta, rstest, …) unless asked.
- **Dependency style**: crates.io major pins only, no git/path deps. Notable: `reqwest` `default-features = false`, features `json`, `rustls-tls`, `blocking` (no native-tls); `rusqlite` `bundled` (do not switch off casually); `zip` `deflate` only (EPUB/DOCX assume it); `serde_json` `preserve_order`. HTTP is deliberately rustls + blocking; do not add tokio as a first-class API (it is a transitive reqwest dep only). `Cargo.lock` is committed; its `[[package]] name = "attx"` entry may lag `Cargo.toml` (lock still lists 0.7.0).
- **Release profile**: `lto = true`, `codegen-units = 1`, `strip = true`.
- **Config keys** (`setting.example.toml` == `Settings` defaults):
  - `[llm]` `default_client` + `[[llm.clients]]` (`name`, `provider_type` default `"openai"`, `base_url`, `api_key`, `model`, `timeout` default 600; optional `temperature`, `reasoning_effort`, `max_tokens`, `stream`, `extra` table merged last into the request; `extra` cannot replace `messages`; `stream=true` parses SSE `delta.content`)
  - `[translation]` `worker_count=8`, `rpm=60` (`0` = unlimited), `retry_count=3`, `retry_delay=2`, `batch_chars=2500`, `max_context_items=6`
  - `[glossary]` `enabled=false`, `method=llm|stats` (parse also accepts `stat`/`regex` → Stats), `min_occurrences=10`, `max_terms=200`, `inject_limit=30`
  - `[learn]` `auto_summarize=true`, `llm_review=false`
- **Env**: `ATTX_HOME` is config **if** `$ATTX_HOME/setting.toml` exists, and is the first-choice dir for `profiles/` and `knowledge/`. Fallback for those two only: `dirs::config_dir()/attx/{profiles,knowledge}` (Linux `~/.config/attx/...`). Neither ATTX_HOME nor config dir → error asking to set `$ATTX_HOME`. Writes use the first listed dir even if it does not exist yet (`create_dir_all`).
- **gitignore**: never commit `setting.toml` (API keys), `.attx/`, `.attx-*/`, `*.db`, `*.attxbak`, `*.epub`, `*.zh.txt`, `*.zh.json`, `/site/`, `/target`. Commit `setting.example.toml` only. `profiles/examples/*.toml` are templates, not compiled in.
- **Docs**: MkDocs Material, folder i18n `docs/{en,zh,ja}/` (en default). Build must pass `--strict`. Do not edit `site/`.

Skill-doc drift to ignore when coding: Skill wizard text still says `worker_count=4` and a "v0.4+" banner; **shipped default is 8**, crate is **0.8.1**. Skill pending-size warning is `>2000`; `agent-usage.md` says `>5000` (prefer 2000). Skill principle 7 says only rmmz writes in place; `overwrite=true` profiles do too. `docs/*/agents.md` lists only a 4-item hard-stop subset. Skill file-workspace ".文件名" is imprecise; code uses `.attx-<stem>/`.

## Testing & QA

- **All tests are inline `#[cfg(test)] mod tests` at file bottom** in `src/`: no `tests/` directory, no `[[test]]`, no integration crate. 142 `#[test]`s across 24 files.
- **Style**: snake_case behavior names (often `*_roundtrip`); `assert` / `assert_eq`; std + production deps. Shared fixture `adapter::test_dir` → `temp_dir()/attx-test-{pid}-{name}` (`create_dir_all`, no cleanup). Zip adapters build fixtures with `zip::ZipWriter` (EPUB must keep `mimetype` stored/first). Some glossary / knowledge / rmmz_plugins tests use `attx-gl-{pid}`, `attx-kn-{pid}`, `attx-pl-{pid}` and `remove_dir_all`.
- **Coverage that exists**: adapters dominate (extract → fake `Translation` map → writeback). `glossary.rs` (mine/select/toml/info/namebox; not `build` LLM), `knowledge.rs` (including every embedded default parses), `learn.rs` (propose/upsert plus `learn note` workspace I/O), `preserve.rs` (mask/roundtrip), `review.rs` (residual/identical/namebox), `profile.rs` (including `repo_example_profiles_compile` walking `profiles/examples/`), `textio.rs` (`decode_bytes` only), `config.rs` (TOML parse, not `load`/`resolve_config_path`), `model.rs` (mask + ja detect), `llm.rs` (`extract_json_span` + neighbor map).
- **Untested modules** (regressions show only via CLI smoke): `pipeline.rs`, `store.rs`, `quality.rs`, `main.rs`, `adapter/jsonl.rs`. When changing those, run `doctor` / `--help` and ideally a tiny extract → fake-translate → writeback fixture. `cargo test` will stay green while they regress. `Translator`, `RateLimiter`, HTTP, `learn::{summarize,review}`, and ATTX_HOME profile I/O are also untested. New logic in those files belongs in the same file's `#[cfg(test)] mod tests`.
- **Parallelism hazard**: `knowledge::file_missing_or_broken_degrades_to_empty` does `unsafe { set_var("ATTX_HOME", ...) }`. Pid-keyed shared temp dirs collide if two tests in the same process share them. Use `-- --test-threads=1` when touching ATTX_HOME or those dirs. CI does **not** serialize (`cargo test` default threads).
- **QA expectation**: no coverage thresholds, no doctests. Keep new logic in the same style (inline unit test, temp-dir fixture). Do not add a `tests/` crate unless asked. Do not hit the network in unit tests.
