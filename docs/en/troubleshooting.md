# Troubleshooting

Read the final stdout JSON and exit code together. Batch progress on stderr is not completion evidence. Avoid deleting a workspace, changing source files, or relaxing integrity checks as a first response.

## Installation and configuration

| Symptom | Check and next action |
|---|---|
| `attx` not found | Use the extracted binary's path or add its directory to `PATH`; PowerShell current-directory invocation is `.\attx.exe`. |
| Source build rejects edition/MSRV | Use stable Rust 1.89 or newer and an available platform linker. |
| `no LLM clients configured` | Copy the example, edit local endpoint/key/model, and pass the intended `--config`. |
| Client name not found | Match `--client` or `llm.default_client` to a unique `[[llm.clients]].name`. |
| Config parse error | Fix the named TOML syntax/type. Existing malformed settings also affect local commands. |
| Wrong settings appear active | Check explicit config, existing `$ATTX_HOME/setting.toml`, then cwd `setting.toml`; platform profile directories are not settings lookup. |
| `doctor` exits 0 but cannot translate | Read `llm.configured`, `llm.error`, and the actual `ping` string; its top-level status is not a connectivity certificate. |

Never share a real API key or whole private config when asking for help. Endpoint error bodies can contain private provider text; redact before publishing diagnostics.

## Provider errors and cost

400, 401, 403, wrong endpoint/model, and unsupported request fields are fatal configuration issues. Stop and fix the exact cause. `provider_type` does not switch to a native Anthropic/Gemini protocol. `base_url` should not already end in `/chat/completions`.

408, 429, 5xx, and network failures are bounded retries. For persistent 429 reduce workers and request rate, and inspect the provider's quota. Multiple processes have separate limiters. `rpm=0` disables rate limiting; it does not mean zero requests.

Truncation (`finish_reason=length`), malformed JSON/SSE, missing IDs, duplicate/unknown IDs, invalid line counts, and mask damage can consume retry budgets. Reduce batch size or choose a capable model; increase the appropriate output-token limit if the endpoint supports it. Avoid conflicting `max_tokens` and `max_completion_tokens`, and do not force an incompatible JSON-object response mode for a JSON-array translation contract.

Do not keep invoking repair to reset bounded attempts. Disable unexpected paid glossary/model-reviewed learning or use `--no-infer` for format inference; see [configuration cost controls](configuration.md). Earlier committed batches remain cached even if a later request fails fatally.

## Format and extraction

```bash
attx formats
attx detect --input "./input"
attx analyze --input "./input" --src ja
```

No detector match is not proof that the data is translatable by line replacement. Read [profiles](profiles.md) for explicit bounded inference or safe declarative authoring. Binary/encrypted/complex escaped inputs need an external extractor and JSONL writer.

Zero extraction can mean the source tag is wrong, text is ambiguous to script heuristics, the selected adapter covers only part of a format, or learned rules removed units. `run` stops on zero units rather than declaring a translation complete.

```bash
attx status --workspace "./workspace"
attx learn list --workspace "./workspace"
attx learn list --format rmmz
attx extract --workspace "./workspace" --no-knowledge
```

Use the no-knowledge escape hatch only to diagnose filtering. It cannot make a profile/adapter extract never-selected text. Spreadsheet inline strings/formulas, unsupported PO plurals, or arbitrary Ren'Py/Python are examples of real format limits, not necessarily model failures.

For Auto directory input, read supported/copied/unsupported/excluded coverage. Copying an archive or script unchanged is not its translation. Auto is not a directory dispatcher through every adapter. An unrepresentable target in Shift-JIS/another legacy encoding fails safely; convert a copy to UTF-8 and use a new workspace rather than allowing lossy replacement.

## Profile failures

- Existing `profile new`/`infer` output is refused: choose a new path, not an accidental overwrite.
- Unknown TOML keys, missing `text` capture, invalid Rust regex, or no rules: fix the profile schema.
- No-op inference render changed decoded source: the candidate is not safe enough; author a precise profile or external adapter.
- ASCII-quoted/backtick capture or programming escape rejected: no escape strategy exists for that grammar. Use a real parser/adapter instead of broad regex insertion.
- Target delimiter/newline corruption: correct the text within its format contract or use an adapter that encodes the target safely.
- Workspace profile digest changed: restore the original snapshot or create a new workspace for changed rules. Do not flip inferred `overwrite=false` after initialization.

`profile test --roundtrip` is an in-memory marker render, not a translated artifact or a complete grammar proof.

## Workspace busy, stale source, and writeback failure

A busy workspace has a live mutating command. Let it finish. OS locks release on crash; the mere existence of `.attx.lock` does not mean a stale lock needs deletion. `run` keeps one operation lock across stages.

Identity mismatch means a workspace belongs to another canonical input, engine, language pair, or profile. Use `--workspace` with a new directory. Do not edit `workspace.json`/SQLite to defeat this check.

Source changed at an anchor, new/removed source text, event topology changes, or plugin identity/nesting changes require source refresh:

```bash
attx extract --workspace "./workspace"
attx --config "./setting.toml" translate --workspace "./workspace"
attx review --workspace "./workspace"
attx writeback --workspace "./workspace"
```

Inspect the actual change first. For RMMZ, original data/backups provide source evidence while safe unrelated live values remain. Conservative structural differences can require re-extraction even if the visible dialogue looks unchanged.

A backup failure is a hard stop, not a warning to ignore. Check permissions, disk space, and whether `.attxbak` is a regular file rather than a directory/symlink. Do not delete backups automatically. Artifact replacement is atomic per file, so a late multi-file failure can leave earlier outputs replaced. Inspect reported paths and backups before recovery; keep an independent pristine input copy for important work.

## Incomplete reports and partial output

`needs_attention` with exit 2 means unresolved/pending work is visible. Default writeback `blocked` with exit 2 means no normal output was written. Read `skipped_note`, review buckets, and counts:

```bash
attx review --workspace "./workspace"
attx --config "./setting.toml" repair --workspace "./workspace" --dry-run
attx --config "./setting.toml" repair --workspace "./workspace"
```

A dry-run can exit 0 while the plan still says blocked. It does not fix anything. Explicit `--allow-partial` writes valid units and keeps/restores originals for unresolved text, still exiting 2. Use it only when that partial result is actually acceptable.

Chinese kana cleanup is narrow and does not delete full Japanese passages. Do not add broad preserve regexes or remove text to make residual counters zero. Glossary substring misses are advisory; inspect inflection before rewriting a correct term.

RMMZ overflow is reported rather than truncated. A 44-halfwidth-cell threshold cannot predict every font/plugin layout. Inspect the real game and correct text through JSONL so the cache stays aligned with written data.

## JSONL import rejected

Preserve the export's `id` and exact source `text`. Unknown IDs, source mismatch, duplicate aliases for one unit, malformed records, invalid line counts, or protected-content loss/order errors reject the entire import before commit. Use a fresh `--filter all` export when source changed.

If both target fields exist, `translation_lines` wins. Missing/empty target fields are audit records and do not increase imported count. Import success is not artifact writeback; review and writeback are separate commands. Standalone `translate-jsonl` can write incomplete records with exit 2, so validate them before any external writer applies them.

## Reporting a problem

Include version/platform, command with sensitive paths/credentials redacted, engine/workspace language pair, exit code, relevant final JSON fields, and a small synthetic reproduction. For writeback say whether it was actual, dry-run, blocked, or partial. For Auto include coverage; for RMMZ include layout counters and domain/location samples.

Do not attach private settings, real keys, full copyrighted source works, or an entire user cache by default. If semantic quality is the issue, show a permitted short source/target pair and the desired correction rather than claiming a mechanical counter proves the model wrong.

Related pages: [CLI](cli.md), [quality and repair](quality.md), [workspaces](workspace.md), and [development](development.md).