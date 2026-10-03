# Development

attx is a Rust 2024 binary crate, currently 0.10.1, with MSRV 1.89. Start with [architecture](architecture.md) and reuse the existing adapter/pipeline split. Format work belongs in extraction/rendering; network, retries, cache persistence, and durable artifact writes stay shared.

## Source map and setup

```bash
git clone https://github.com/kaecho/attx.git
cd "attx"
cargo build --locked
```

| Change | Main files |
|---|---|
| CLI flag/subcommand/report dispatch | `src/main.rs` |
| Settings schema/defaults/example | `src/config.rs`, `setting.example.toml` |
| Pipeline/identity/repair/writeback | `src/pipeline.rs` |
| Format implementation/registry | `src/adapter/<format>.rs`, `src/adapter/mod.rs` |
| Unit/translation contracts | `src/model.rs` |
| SQLite transactions/cache/published state | `src/store.rs` |
| HTTP/prompt/batching/context/retries | `src/llm.rs` |
| Protection/quality/review | `src/preserve.rs`, `src/quality.rs`, `src/review.rs` |
| Custom profile grammar/rendering | `src/profile.rs`, `profiles/examples/` |
| Terms/experience | `src/glossary.rs`, `src/knowledge.rs`, `src/learn.rs`, `src/defaults/` |
| Encoding/durable file operations | `src/textio.rs`, `src/fileio.rs` |
| Human/agent contracts | `docs/en/`, `docs/zh/`, `docs/ja/`, README files, `skills/attx/` |
| CI/docs/release | `.github/workflows/`, `mkdocs.yml`, `requirements-docs.txt` |

Dependencies include Clap, Serde/JSON/TOML, regex, SHA-256, bundled rusqlite, blocking reqwest with Rustls, ZIP, encoding detection/encoding, directory walking, `cap-std` 4 directory capabilities, and `same-file` 1 identity handles. There is no asynchronous runtime or external database service.

## Add an adapter

An adapter implements `FormatAdapter: Send + Sync`. It can read input and build output bytes, but must not make HTTP calls, mutate workspace state, write translated destination files, or run generated code. Return `OutputFile` values so the shared pipeline owns staging, backups, source/output permissions, and replacement.

The following complete example handles a deliberately narrow UTF-8 `.greeting` format containing one line of visible text. It demonstrates the boundary, not support for an arbitrary script grammar. Add it as `src/adapter/greeting.rs` and register it only if that format is wanted.

```rust
use super::{FormatAdapter, OutputFile, output_sibling};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use anyhow::{Result, bail};
use std::{collections::BTreeMap, path::Path};

pub struct GreetingAdapter;

fn read(input: &Path) -> Result<(String, &'static str)> {
    let mut body = std::fs::read_to_string(input)?;
    let ending = if body.ends_with("\r\n") {
        body.truncate(body.len() - 2);
        "\r\n"
    } else if body.ends_with('\n') {
        body.pop();
        "\n"
    } else {
        ""
    };
    if body.contains(['\r', '\n']) {
        bail!("greeting requires exactly one text line");
    }
    Ok((body, ending))
}

impl FormatAdapter for GreetingAdapter {
    fn id(&self) -> &'static str { "greeting" }
    fn label(&self) -> &'static str { "Single-line greeting" }
    fn extensions(&self) -> &'static [&'static str] { &["greeting"] }

    fn extract(&self, input: &Path, src: &str) -> Result<Vec<TextUnit>> {
        let (text, _) = read(input)?;
        if text.trim().is_empty() || !needs_translation(&text, src) {
            return Ok(Vec::new());
        }
        let lines = vec![text];
        Ok(vec![TextUnit {
            id: TextUnit::compute_id(self.id(), "greeting", &lines),
            engine: self.id().into(),
            domain: "text".into(),
            location: "greeting".into(),
            item_type: ItemType::ShortText,
            role: String::new(),
            original_lines: lines,
            source_line_paths: Vec::new(),
            context: "greeting".into(),
            payload: String::new(),
        }])
    }

    fn writeback(
        &self, input: &Path, dst: &str, units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        let (original, ending) = read(input)?;
        if units.len() > 1 {
            bail!("greeting supports one source unit");
        }
        let replacement = if let Some(unit) = units.first() {
            if unit.location != "greeting" || unit.original_lines.as_slice() != std::slice::from_ref(&original) {
                bail!("greeting source anchor changed");
            }
            translations.get(&unit.id)
        } else {
            None
        };
        let mut text = if let Some(tr) = replacement {
            if tr.translation_lines.len() != 1 || tr.translation_lines[0].contains(['\r', '\n']) {
                bail!("greeting translation requires one line");
            }
            tr.translation_lines[0].clone()
        } else {
            original
        };
        text.push_str(ending);
        let mut output = OutputFile::text(output_sibling(input, dst, "greeting"), text);
        output.permissions = Some(std::fs::metadata(input)?.permissions());
        Ok(vec![output])
    }
}
```

Add `pub mod greeting;` and `Box::new(greeting::GreetingAdapter)` to the registry at the appropriate priority, before Auto. Extension-free directory adapters should override detection/input kind. Avoid broad detection that steals inputs from more specific adapters. Stable IDs, locations, grouping, source hashes, and payload anchors must remain deterministic.

The example uses one unit; larger adapters should avoid unnecessary whole-file copies, per-record regex compilation, and duplicated parsing. Keep original nontext bytes or parse/serialize with a defined fidelity contract. Validate source anchors and safely encode target syntax rather than concatenating unescaped text into a programming language.

Untranslated units must retain original text. In-place writers must account for repeated writeback and previously published translations; a backup is extraction evidence, not a replacement for all live nontext edits. Provide layout/overflow signals instead of dropping text to fit fixed slots.

## Add or change a profile

Use [profile authoring](profiles.md) before writing code when the grammar is genuinely simple. Add a concrete example under `profiles/examples/` with narrow extensions, selection rules, and notes explaining machine-text exclusions. Include extraction and marker/no-op cases, CRLF, negative matching, and delimiter/escape hazards.

Profiles use Rust regex named `text` and optional `role` groups, or JSON key/path rules. Inferred line rules must anchor both ends. ASCII-quoted/backtick code captures and programming escapes are rejected without a declared escaping strategy; do not bypass that safety check to increase apparent coverage. Target newline/delimiter corruption must fail.

Profile snapshots are workspace-bound. Updating an extraction grammar requires a new workspace, not editing its copied TOML and hoping old anchors still fit. Unknown fields are rejected, so adding profile schema fields means updating deserialization, validation, template, examples, inference prompt/schema, tests, and docs together.

## Add a CLI/configuration option

1. Define the field/subcommand in `main.rs` with its required/optional/default behavior and conflict rules. Forward it to the owning module; do not leave an accepted no-op flag.
2. For persistent settings, add typed Serde fields and defaults in `config.rs`, including `Default` behavior for missing sections.
3. Update the generated example and `setting.example.toml` together. A test checks that they agree and parse.
4. Keep optional HTTP request fields distinct from application policy. Define named-field versus `extra` precedence and whether the provider actually supports the field.
5. Add parsing/default/migration tests and behavioral cases. If a report changes, update the final stdout contract and incomplete/fatal exit handling.
6. Update all three locale manuals, both READMEs where applicable, the skill/reference contracts, and release notes. Preserve the shared page filenames and sentence-case English headings.

When an exported/internal shared symbol changes, inspect its references before migration. Change every caller rather than leaving compatibility aliases as a second convention. Avoid logging secrets and unbounded sample payloads in new diagnostics.

## Test strategy

Tests belong beside the implementation. Use small synthetic fixtures, not copyrighted/private work or a user's real translation cache. Keep normal tests offline.

- Adapter tests: detection positive/negative cases, exact unit IDs/locations/roles, missing translations, no-op render, translated render, escaping, source changes, and unrelated-field fidelity.
- Text tests: UTF-8/UTF-16/legacy encoding, BOM and CRLF, malformed/lossy text, unrepresentable target text for Auto.
- Model tests: reordered/duplicate/unknown/omitted IDs, line counts, protected multiplicity/order, malformed JSON, truncated JSON/SSE, and empty content.
- Request tests: a local mock provider for 408/429/5xx bounded recovery, fatal 400/401/403 stop, failed-only narrower retries, rate starts, and saved earlier batches after a later failure.
- Workspace tests: identity conflicts, copied-profile digest, complete source snapshot additions/removals, import transaction rejection, repeat writes, published-line tracking, busy locks, and crash-released locks.
- File tests: backup failure, first-backup preservation, symlink/nonregular rejection, staging cleanup, permission inheritance, anchored-directory substitution, and per-file partial failure.
- Game tests: stable event command count, name-plus-SE/control ordering, first speaker slot, overflow preservation, plugin nesting, source/live topology, repeat partial restore, and no plugin-source writes.

Suggested maintainer checks after all changes land:

```bash
cargo test --locked -- --test-threads=1
cargo build --release --locked
./target/release/attx --help
./target/release/attx doctor --json
```

A local mock-provider smoke and parse/re-open checks for actual generated artifacts add behavioral evidence. A successful dry-run alone is insufficient. These commands are guidance, not a claim that every current platform or feature was tested by reading this page.

## Documentation and internationalization

The MkDocs Material site uses folder-based i18n: `docs/en`, `docs/zh`, and `docs/ja`. All locales share the same 15 filenames, from `index.md` through `troubleshooting.md`, with navigation in `mkdocs.yml`. Relative manual links stay within the locale. Repository source links use stable GitHub URLs.

```bash
python -m venv ".venv-docs"
. ".venv-docs/bin/activate"
pip install -r "requirements-docs.txt"
mkdocs serve
mkdocs build --strict
```

PowerShell activation:

```powershell
python -m venv ".venv-docs"
& ".\.venv-docs\Scripts\Activate.ps1"
pip install -r ".\requirements-docs.txt"
mkdocs build --strict
```

Build all languages in strict mode. Preview navigation, code blocks, cross-links, and search; a strict build is not a visual or factual review. Do not manually edit generated `site/` as a source change. The docs workflow builds and deploys GitHub Pages on relevant main/master changes.

## CI and release

Ordinary CI builds release binaries, runs tests, and smoke-checks the CLI on Ubuntu and Windows. Release tags matching `v*` or manual dispatch run a verification job first: locked tests, release build, tag/version agreement, CLI help/doctor, and strict docs. The target matrix then builds locked packages:

| Platform | Rust target |
|---|---|
| Linux x86_64 | `x86_64-unknown-linux-gnu` |
| Windows x86_64 | `x86_64-pc-windows-msvc` |
| macOS ARM64 | `aarch64-apple-darwin` |
| macOS x86_64 | `x86_64-apple-darwin` |

Windows packages are ZIP; other packages are tar.gz. Packages include the binary, English/Chinese README, changelog, license, example settings, `skills/`, `profiles/`, `docs/`, `mkdocs.yml`, and docs requirements. Publishing waits for the matrix and attaches its packages to the tag release.

For 0.10.1, package version and release tag must agree (`0.10.1`, `v0.10.1`). Update Cargo metadata/lockfile, changelog, source examples, CLI/skill contracts, and translated manuals before tagging. Do not publish secrets, user workspaces, generated private JSONL, or throwaway fixtures. The workflow definition is [release.yml](https://github.com/kaecho/attx/blob/main/.github/workflows/release.yml), not a promise that a future run cannot fail.

## Security invariants and design attribution

Keep adapters network-free and the runtime synchronous. Bound attempts, context, samples, and optional model work. Treat source samples/model output as untrusted data; never run generated extraction code. Protect placeholder order/multiplicity and source identity. Default incomplete writeback remains blocked; normal authorized writeback has no mandatory second permission ceremony. Backups must succeed before replacement, and directory output must not be described as a transaction.

LinguaGacha's [inspected commit](https://github.com/neavo/LinguaGacha/tree/5a0058d57abfc6df371339818894097f3879bd28) informed reliability concepts such as bounded failed-unit retry and shared batch/agent validation. [Architecture](architecture.md) links the specific source evidence. Implementations here are independent Rust code. Do not copy upstream source or assume attx's MIT license grants rights to it: the inspected upstream README requires attribution and commercial authorization, and no root license was observed. Auto/profile inference is attx's own extension, not an upstream licensing or capability claim.

Related pages: [configuration](configuration.md), [profiles](profiles.md), and [troubleshooting](troubleshooting.md).