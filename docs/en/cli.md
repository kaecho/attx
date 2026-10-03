# CLI reference

```text
attx [--config <path>] [--client <name>] <command> [options]
```

Commands and subcommands also support `-h`/`--help`; the root supports `-V`/`--version`. `--config` and `--client` are global and can appear after a subcommand. All examples use explicit long options. Source definitions: [src/main.rs](https://github.com/kaecho/attx/blob/main/src/main.rs).

## Exit codes and output channels

| Code | Meaning |
|---|---|
| `0` | Requested operation completed, or a dry-run plan was returned |
| `1` | Fatal application error, reported as `error: ...` on stderr |
| `2` | Structured incomplete/blocked translation or writeback result; also CLI argument parsing errors |

Translation, repair, standalone JSONL translation, and run/writeback print their final JSON before incomplete exit 2. A parser error does not have that result JSON. Fatal failures do not promise a complete JSON object. Keep stdout and stderr separate: batch progress, warnings, and diagnostic errors are stderr, not the final result.

Most commands return JSON. `doctor` is human-readable unless `--json` is supplied; `learn defaults` emits TOML; help/version are text. `status`, `review`, and glossary advisory checks can exit 0 while their counts show outstanding work. `doctor --json` can report top-level `status="ok"` even when the selected client is missing or the ping fails, so inspect its nested fields.

A dry-run exit 0 means a plan was inspected, not that source/cache/artifact files were written. There is no global dry-run flag and no `run --dry-run`.

## Global options

| Option | Meaning |
|---|---|
| `--config <path>` | Explicit settings file; otherwise existing `$ATTX_HOME/setting.toml`, then cwd `setting.toml` |
| `--client <name>` | Overrides `[llm].default_client` for this invocation |

`--input` also accepts `--game` on `detect`, `analyze`, `init`, `run`, `profile infer`, and `profile test`. This alias does not turn a file adapter into a game-directory adapter. Source defaults to `ja`, target to `zh` wherever those options exist.

## Discovery and health

### `doctor [--ping] [--json]`

Checks selected client configuration and inventories adapters/saved profiles. `--ping` sends one small model request. Can create `setting.example.toml` in the working directory if absent. JSON fields are `llm`, `ping`, `adapters`, `saved_profiles`, and `status`. `llm` contains `configured` and, when selected, client `name`, `model`, `base_url`; failures include `error`. It never intentionally prints the API key. Ping is `skipped`, `ok: ...`, or `error: ...`.

### `formats`

Returns `formats[]` with `id`, `label`, `extensions`, and `input` (`file`, `directory`, or `file|directory`). Saved custom profiles also include their profile path. See [formats](formats.md) for coverage limits.

### `detect --input <path>`

Returns `engine`, `content_root`, `label`, and `profile` (null without a profile). Dedicated adapters precede saved profiles, then Auto. `detect` does not accept `--engine`; force during `init` or `run`.

### `analyze --input <path> [--src ja]`

Returns input/language context, `builtin_detect`, `saved_profile_detect`, `details`, and `next_steps`. File details can include size, binary/container clues, encoding/lossiness, line counts, source-language line counts, JSON shape, and `sample_head`. Samples are capped at 40 nonempty lines and 160 characters per line; JSON top keys at 20. Directory details include files, extension counts, at most 15 sample paths, and a representative peek. Directory analysis scans to depth 6 and is not a full extraction contract.

## Profiles

| Command | Options and defaults |
|---|---|
| `profile new` | Required `--output <path>`; `--name myformat` |
| `profile infer` | Required `--input <path> --output <path>`; `--src ja --name auto-profile` |
| `profile test` | Required `--profile <path-or-name> --input <path>`; `--src ja --limit 10`; optional `--roundtrip` |
| `profile save` | Required `--profile <path>`; optional `--force` |
| `profile list` | No command-specific options |

`new` returns `written`, `next`, `status` and refuses an existing output. `infer` costs requests and returns `profile`, `engine`, `units`, `attempts`, `output`, `roundtrip=true`, `overwrite=false`, `status` when validated. It permits at most three attempts and never overwrites existing output or executes generated code.

`test` returns `profile`, `engine`, `units`, `sample[{location,role,text}]`, and `detects`. `--limit` caps displayed samples with effective minimum 1; it does not limit extraction. `--roundtrip` adds in-memory render details with `ok`, `output_files`, and output samples or an error. A false nested roundtrip result must not be ignored because the process itself returned successfully.

`save` returns `saved` and `status`; `--force` allows replacement of a same-name saved profile. `list` returns `profiles[{name,engine,label,extensions,path}]` and searched `dirs`. Workspace profile snapshots cannot be modified to change an initialized grammar. See [profiles](profiles.md).

## Workspace pipeline

### `init --input <path> [--engine <id>] [--profile <path-or-name>] [--src ja] [--dst zh] [--workspace <dir>]`

Registers or reopens a workspace. `--profile` selects a profile and takes precedence when supplied; otherwise `--engine` forces an adapter, including saved `custom:<name>`. Omitted selection uses detection. Returns `workspace` and `status`. Input/engine/language/profile identity cannot change in an existing workspace.

### `extract --workspace <dir> [--no-knowledge]`

Extracts and refreshes source units, keeping only source-matching cache entries. `--no-knowledge` bypasses learned filtering for this extraction. Returns `extracted`, `skipped_by_knowledge`, `rules_applied`, `status`, and for Auto `auto_coverage`. Coverage fields are `supported_files`, `copied_files`, `unsupported_total`, `unsupported_paths`, `excluded_entries`, and `excluded_paths`; each path-sample array caps at 50. Supported/copy/excluded counts are not successful translation counts.

### `translate --workspace <dir> [--limit N] [--dry-run] [--retry-passthrough]`

Translates pending/mechanically flagged entries with bounded retries and extra repair. `--limit` caps selected units. `--retry-passthrough` explicitly requeues failed source placeholders; dry-run does not persist that requeue. Accepted batches save incrementally. `--dry-run` makes no model calls or translation-cache edits.

### `repair --workspace <dir> [--limit N] [--dry-run]`

Targets the shared review candidate set: pending, passthrough, residual source, control/name consistency findings. Uses at most `max(1, repair_rounds)` passes. Advisory glossary substring misses are not a reason for endless retranslation. Returns the same report type as `translate`.

### `writeback --workspace <dir> [--dry-run] [--no-learn] [--allow-partial]`

Normalizes in memory, checks completeness/source identity, renders/stages artifacts, checks backups, and replaces per file. `--dry-run` plans only. `--no-learn` skips this invocation's post-writeback experience summary. `--allow-partial` writes valid entries and retains/restores original source for unresolved entries; incomplete output remains exit 2.

Default incomplete writeback reports blocked with no paths/files and makes no output/cache mutation. Actual successful writeback persists normalized/reflowed cache text. There is no mandatory interactive writeback permission prompt.

### `run --input <path> [--engine <id>] [--profile <path-or-name>] [--src ja] [--dst zh] [--workspace <dir>] [--limit N] [--no-translate] [--no-writeback] [--glossary | --no-glossary] [--no-infer] [--allow-partial]`

Runs detection/init, extraction, optional glossary, translation/repair, and optional writeback under one workspace lock. Reuses copied profiles; unmatched safe text can use bounded paid profile inference.

- `--no-translate` performs extraction without model translation or normal writeback; it also suppresses automatic inference and run-time glossary construction.
- `--no-writeback` retains translated cache without artifact output.
- `--glossary` forces optional paid construction; `--no-glossary` suppresses it. These two conflict.
- `--no-infer` suppresses paid profile inference, not local Auto detection.
- `--allow-partial` deliberately permits incomplete artifacts with exit 2.

Returns `workspace`, `extracted`, `extract`, `status`, and each executed stage such as `profile_inference`, `glossary`, `translate`, `review`, and `writeback`. Optional ordinary glossary errors appear under `glossary.error`; fatal credentials/configuration errors stop the run. `--limit N --no-writeback` is a trial, not a full-run success.

### `status --workspace <dir>`

Returns `engine`, `game_path`, `source_lang`, `target_lang`, `total`, `translated`, `pending`, `passthrough`, and `domains` mapping each domain to `total/translated`. The historical field name `game_path` also describes non-game input. Passthrough is not successful translation. Status counters alone are not a semantic review.

### `review --workspace <dir>`

Returns overall `total/translated/pending/passthrough`, `glossary`, and quality buckets documented below. No model requests. It reports findings without fixing them or returning an incomplete-operation exit code.

## Translation report fields

Used by `translate`, `repair`, and `translate-jsonl`; `run.translate` embeds it.

| Field | Meaning |
|---|---|
| `pending_before` | Pending source units before work |
| `translated` | Newly successful pending units, or standalone successful count; excludes passthrough |
| `pending_after` | Remaining without translations |
| `passthrough` | Cached/returned source placeholders |
| `dry_run` | Whether this is planning only |
| `skipped_note` | Explanation of skipped/unresolved work |
| `status` | `ok` or `needs_attention` |
| `planned` | Units selected within the limit |
| `repaired` | Initially flagged units successfully repaired |
| `repair_rounds` | Extra/explicit repair passes executed |
| `unresolved` | Mechanical repair-candidate count after work |
| `normalized_lines` | Lines changed by normalization on this path |
| `review` | Full mechanical review report |

Standalone JSONL uses no workspace repair passes, so `repaired` and `repair_rounds` are zero. Its output file can still contain unresolved records.

## Writeback report fields

| Field | Meaning |
|---|---|
| `files`, `units_applied`, `paths` | Written artifacts/applied units; planned values during dry-run |
| `dry_run` | True means no artifact or normalized-cache write |
| `status` | `ok`, `blocked`, or `needs_attention` |
| `skipped_note` | Explanation when units cannot be applied |
| `units_skipped` | Pending/invalid units omitted from the applied subset |
| `normalized_lines` | Chinese-target normalization signal |
| `reflowed_units` | RMMZ fixed-slot layout changes |
| `overflow_lines` | Unavoidable layout overflow, not silently truncated text |
| `review` | Mechanical report using normalized candidate text |
| `learned` | Optional actual post-writeback summary report |

Read `dry_run` with the counts; the same path array can describe a plan rather than an actual write. Atomic replacement is per file, not a directory transaction.

## Review report fields

Each bucket is `{count, sample:[{location,unit_id,detail}]}`, with at most 40 samples. Buckets are `residual_source`, `kana_edge`, `kana_mixed`, `kana_untranslated`, `identical`, `control_loss`, and `namebox_mismatch`. The kana buckets are disjoint, but aggregates and other categories overlap. Do not sum them as a unique-unit count.

`glossary` contains `active_terms`, `terms_seen`, `terms_fully_applied`, and `violations[{src,dst,occurrences,applied}]`. These substring warnings are advisory. Repair selection is not limited to the visible sample arrays. Full text requires JSONL export.

## Preserve commands

| Command | Options |
|---|---|
| `preserve list` | Required `--workspace <dir>` |
| `preserve add` | Required `--workspace <dir> --pattern <regex>`; `--info ""` |
| `preserve remove` | Required `--workspace <dir> --pattern <regex>` |

List returns `engine`, `count`, and `rules[{pattern,info,source}]`. Add returns `added`, `file`, `status`; remove returns `removed`, `pattern`, `status`. Removal matches the exact workspace pattern and does not remove built-ins. Bad/empty-match regexes are rejected. See [preserve examples](usage.md).

## JSONL commands

| Command | Options |
|---|---|
| `translate-jsonl` | Required `--input <file> --output <file>`; `--src ja --dst zh`; optional `--limit N` |
| `export-jsonl` | Required `--workspace <dir> --output <file>`; `--filter pending` |
| `import-jsonl` | Required `--workspace <dir> --input <file>` |

Export filters are `pending`, `all`, `translated`, and `passthrough`; bad values are errors. Export returns `exported` and `output`; import returns `imported` and `status`. Standalone translation returns the translation report, writes visible incomplete records when necessary, and does not have workspace resumption.

Record schema, exact-source/duplicate/control validation, array precedence, and transactional import are described in [workspaces and JSONL](workspace.md). Input/output must differ for standalone translation.

## Glossary commands

| Command | Options |
|---|---|
| `glossary build` | Required `--workspace <dir>`; optional `--min-occurrences N --dry-run` |
| `glossary list` | Required `--workspace <dir>`; optional `--all` |
| `glossary add` | Required `--workspace <dir> --src <term> --dst <target>`; `--info ""`; optional `--case-sensitive` |
| `glossary remove` | Required `--workspace <dir> --src <term>` |
| `glossary import` | Required `--workspace <dir> --file <json>` |
| `glossary export` | Required `--workspace <dir> --file <json>` |
| `glossary check` | Required `--workspace <dir>` |

Build makes paid requests unless dry-run, regardless of `[glossary].enabled`. Its report includes `candidates`, `above_threshold`, `truncated`, `asked`, `added`, `rejected`, `total_active`, `min_occurrences`, `dry_run`, `file`, and optional `sample`. `asked` describes batch scale; `truncated` counts terms dropped by `max_terms`, not source coverage beyond the 40-batch cap.

List returns `source_lang`, `target_lang`, `count`, and `terms`, with `--all` adding rejected terms. Term fields are `src`, `dst`, `info`, `count`, `status`, `source`, and `case_sensitive`. Add returns `added/file/status`; remove returns `removed/src/status`; import returns `imported/status`; export returns `exported/file/status`. Check returns the glossary review shape; warnings do not trigger exit 2, and absent glossary is an error. See [glossary schemas](usage.md).

## Learning commands

| Command | Options |
|---|---|
| `learn summarize` (alias `scan`) | Required `--workspace <dir>`; optional `--llm` |
| `learn note` | Required `--text <instruction>` and a scope `--workspace <dir>` or `--format <id>`; `--topic prompt --name ""` |
| `learn pending` | No command-specific options |
| `learn review` | `--approve 1,3`, `--reject 2`, and/or `--approve-all` |
| `learn list` | Optional `--format <id> --workspace <dir>` |
| `learn defaults` | Required `--format <id>` |
| `learn forget` | Exactly one of `--field <field>` or `--name <name>`; optional `--format <id> --workspace <dir>` |

`summarize` returns `format`, `units_scanned`, `fields_seen`, `entries_written`, `pending`, `notes`, and `file`. Ordinary summarization is local; `--llm` requests paid review. `note` returns `format/topic/name/source/text/file/layer/reaches_prompt`. If both workspace and format are supplied, the workspace is the write destination and the explicit format must match its experience identity; use one scope to keep intent clear.

`pending` returns count and entries with current 1-based indices. `review` returns `approved/rejected/remaining/files_written`. Approval can activate text-removing skip rules; agents must not run `--approve-all` on their own. No selections is an error.

`list` returns `formats`, `knowledge`, and an explanatory note; built-in defaults are not included. `defaults` emits embedded TOML and errors when a format has no embedded defaults. `forget --field` edits global extraction experience and cannot use `--workspace`; `forget --name` can use workspace scope. Forget reports count, selected field/name, and status.

For on-disk schemas and safe precedence, read [workflows](usage.md). For automation rules, read [agents](agents.md).