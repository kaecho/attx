# Workspaces and JSONL

A workspace is a persistent cache bound to one input, adapter, source language, and target language. It is not a general bag of translations that can be attached to another project.

## Locations and files

Directory inputs default to `<content_root>/.attx`; file inputs default to `<parent>/.attx-<stem>`. A detected RPG Maker content root can be below the original input. Choose `--workspace` if two inputs have the same stem or you want an explicit cache location.

| File | Purpose |
|---|---|
| `attx.db` | SQLite workspace metadata, source units, translations, source snapshot, and published-line tracking |
| `workspace.json` | Readable metadata snapshot, not a replacement for database state |
| `.attx.lock` | Operating-system lock backing file |
| `profile.toml` | Selected/inferred profile snapshot, when applicable |
| `glossary.toml` | Workspace terminology |
| `preserve.toml` | Workspace literal-protection regexes |
| `experience.toml` | Project-specific extraction experience and prompt notes |

Metadata includes engine, original game/input path, resolved content root, source/target languages, and creation timestamp, plus internal safety identity information. Do not change JSON metadata or SQLite rows to bypass identity checks.

## Identity and resumption

```bash
attx init --input "./book.epub" --src ja --dst zh --workspace "./book workspace"
attx extract --workspace "./book workspace"
attx --config "./setting.toml" translate --workspace "./book workspace"
```

Opening an existing workspace requires the same engine, canonical content root, normalized language pair, and copied profile identity. A changed profile or edited snapshot is rejected. Use a new workspace for a new language pair, input, or extraction grammar. Language tags are normalized, but this is not permission to repurpose an existing database.

A unit's identity includes engine, location, and source lines. Translations carry a source hash. Re-extraction updates the complete source set while preserving still-matching translations. Changed/disappeared source cannot retain an old cache entry as current text. Source-set/anchor validation also detects newly added text before writeback: run extraction again after source changes.

Completed batches are saved incrementally. An interrupted or later-failed run keeps earlier committed work. Reuse the workspace and configuration, then inspect status/review before resuming. New source or unresolved entries can still cost requests. A new note does not automatically invalidate accepted entries.

```bash
attx status --workspace "./book workspace"
attx review --workspace "./book workspace"
attx --config "./setting.toml" run --input "./book.epub" --src ja --dst zh --workspace "./book workspace"
```

## Locks and multiple processes

Mutating operations acquire an operating-system workspace lock. `run` holds one lock across its extraction, optional glossary, translation, and writeback stages. A busy error means another mutating command owns that workspace; wait for it rather than starting concurrent repair/import/writeback commands.

The OS releases the lock after a crash. A leftover `.attx.lock` file is not evidence of a live lock and should not be deleted as a routine repair. Reading status does not authorize external database editing. Independent workspaces have independent rate limiters, so concurrent processes can still exceed an account-wide provider quota.

## Backups and atomicity

All artifacts are staged before the pipeline replaces any destination. Existing destinations receive a checked first-write backup by appending `.attxbak` to the complete path: `Map001.json.attxbak`, not `Map001.attxbak`. Existing valid backups are retained rather than replaced on every run.

A backup read/copy/publish failure stops replacement. Temporary staging uses private modes where supported; completed backups and outputs inherit source permissions, so a public source can have a public backup. Directory-handle-anchored operations and parent identity checks prevent path substitution from redirecting staging/backup/replacement. Output symlinks are refused rather than followed.

Each file replacement is atomic. A set of directory outputs is not an all-or-nothing transaction: a later replacement failure can leave earlier files already replaced. Reports/errors distinguish such failures; inspect affected paths and retained backups before resuming. Keep an independent pristine copy for important inputs.

Default incomplete writeback and dry-run do not modify artifact files or persist in-memory normalization/reflow. Successful actual writeback updates changed cache lines to the published text. Published-line tracking permits safe repeated custom in-place repairs without overwriting unrelated live keys/comments. RMMZ source selection uses original data/backups as described in [RPG Maker](rmmz.md).

## JSONL record schema

JSONL uses one JSON object per line. Minimum source record:

```json
{"id":"scene/line/1","text":"こんにちは。"}
```

Supported optional fields are `context`, `role`, `item_type`, `translation`, and `translation_lines`. Item types are `long_text`, `array`, and `short_text`. `translation_lines` represents the structural line array; `translation` is a convenient newline-joined text form.

```json
{"id":"scene/line/1","text":"こんにちは。","context":"scene1","role":"Alice","item_type":"short_text","translation":"你好。","translation_lines":["你好。"]}
```

Workspace export uses human-readable `location` as record `id`. Import accepts that location or the exact internal unit ID for the current unit. The exact `text` must remain current source text. Do not replace source `text` with translation or reinterpret an ID from another workspace.

## Export, review, and import

```bash
attx export-jsonl --workspace "./book workspace" --output "./pending.jsonl" --filter pending
attx export-jsonl --workspace "./book workspace" --output "./full review.jsonl" --filter all
```

Filters:

- `pending` exports units without a cached translation.
- `all` exports all units.
- `translated` selects cached entries, including source placeholders; inspect the workspace report rather than treating this filter as a quality certificate.
- `passthrough` exports cached source placeholders.

Edit only target fields locally, then:

```bash
attx import-jsonl --workspace "./book workspace" --input "./full review.jsonl"
attx review --workspace "./book workspace"
attx writeback --workspace "./book workspace"
```

Import parses and validates the entire file before a transactional translation save. Unknown IDs, exact-source mismatch, malformed records, duplicate references to the same unit, wrong line structure, and protected-literal loss/order errors reject the whole import. A location and internal ID resolving to the same unit count as duplicates even when their strings differ.

Missing/empty translation arrays or missing/empty convenience translations are skipped as audit-only records, not counted as imported. If both representations are present, `translation_lines` takes precedence. Nonempty structurally invalid output is an error. Chinese mechanical cleanup is applied before persistence. Remaining semantic/script problems can be imported for explicit review/repair; successful import alone does not authorize default writeback of invalid text.

`import-jsonl` returns `imported` and `status`; it does not write a translated artifact. Preserve the current export when collecting reviewer edits so exact-source checks remain meaningful.

## Standalone translation

```bash
attx --config "./setting.toml" translate-jsonl --input "./source.jsonl" \
  --output "./translated.jsonl" --src ja --dst zh
```

Standalone JSONL has no workspace cache or resumption. It uses bounded translator retries, not the workspace's extra repair loop. It writes output records even when some remain unresolved/pending, reports those conditions, and returns 2. Existing output gets a checked backup and atomic replacement. The input cannot also be the output.

A limit can leave unprocessed source records in output. Never feed an incomplete JSONL blindly into an external engine writer. Parse the final report and validate records before applying them.

## JSONL as an external format adapter

For a proprietary format, an external extractor provides stable IDs and source text. attx translates those records; an external writer validates source identity, escaping, positions, and target syntax before touching the original container.

If you want workspace resumption, place records in `source.jsonl` inside a directory and initialize that directory with `--engine jsonl`. Its adapter output is `translated.jsonl` in the same directory. A `.jsonl` file workspace uses a language-suffixed sibling instead. This is different from standalone `translate-jsonl`.

JSONL does not add OCR, decrypt a binary, or infer safe engine writeback. Those responsibilities stay with format-specific tooling. Related pages: [formats](formats.md), [profiles](profiles.md), [quality](quality.md), and [CLI](cli.md).