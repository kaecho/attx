# CLI

`attx [--config <path>] [--client <name>] <command> [options]`

Success → JSON on stdout, exit code 0. Failure → `error: …` on stderr, non-zero exit.

The exact stdout JSON shapes are pinned in `skills/attx/references/cli-command-contract.md` — agents should rely on that file plus `--help` of the current binary.

## Global options

| Option | Meaning |
|--------|---------|
| `--config <path>` | Path to `setting.toml` (default: `$ATTX_HOME/setting.toml` → `./setting.toml`) |
| `--client <name>` | Use this `[[llm.clients]]` entry instead of `[llm].default_client` |

`--input` also accepts the alias `--game`.

## Command reference

### `doctor [--ping] [--json]`

Check config and optional LLM connectivity. Plain output is human-readable; `--json` emits `{"llm":{configured,error},"ping","adapters":[],"saved_profiles":[],"status"}` (never the API key).

### `formats`

List supported adapters + saved profiles as JSON: `{"formats":[{"id","label","extensions":[],"input":"file|directory"}]}`. Saved custom profiles appear with `id = "custom:<name>"`.

### `detect --input <path>`

Probe the format. JSON out: `{"engine","content_root","label","profile"}`. Built-in adapters first, then saved custom profiles.

### `analyze --input <path> [--src ja|en]`

Recon report for unknown inputs: `builtin_detect`, `saved_profile_detect`, and for files `details` (size, `binary`, `encoding` incl. Shift-JIS/GBK detection, line counts, JSON shape, `sample_head`) or for directories an extension histogram + a peeked sample file. Ends with `next_steps` suggestions.

### `profile`

| Subcommand | Role |
|------------|------|
| `new --output <path> [--name <name>]` | Write a documented rule template |
| `test --profile <path|name> --input <path> [--src] [--limit 10] [--roundtrip]` | Trial-extract; report matched units (and in-memory writeback with `--roundtrip`) |
| `save --profile <path> [--force]` | Remember the profile in the user profile dir |
| `list` | Saved profiles as JSON |

### `init --input <path> --src ja|en --dst zh [--engine <id>] [--profile <path|name>] [--workspace <dir>]`

Register / open a workspace (creates `attx.db`, `workspace.json`). `--engine` forces a built-in; `--profile` sets a custom one (copied to `<workspace>/profile.toml`, engine `custom:<name>`). Default workspace: `<dir>/.attx` or `<parent>/.attx-<stem>`.

### `extract --workspace <dir> [--no-knowledge]`

Adapter → text units into the DB. JSON out: `{"extracted","skipped_by_knowledge","rules_applied","status"}`. `--no-knowledge` ignores all learned rules (pre-knowledge behaviour).

### `translate --workspace <dir> [--limit N] [--dry-run] [--retry-passthrough]`

LLM over pending units, saved incrementally. JSON out: `{"pending_before","translated","pending_after","passthrough","dry_run","skipped_note"}`. stderr shows `batch i/n` progress. `--dry-run` prints the batch plan without calling the model; `--retry-passthrough` re-queues passthrough units first.

### `writeback --workspace <dir> [--dry-run] [--no-learn]`

Render translated output. JSON out: `{"files","units_applied","dry_run","paths":[]}` — and `learned` when the automatic post-writeback experience summary ran. `--dry-run` plans only; `--no-learn` skips the summary for this run.

### `run --input <path> [--engine] [--profile] [--src] [--dst] [--workspace] [--limit] [--no-translate] [--no-writeback] [--glossary] [--no-glossary]`

init → extract → (glossary if enabled or forced) → translate → writeback, one JSON report. `--glossary` builds a glossary even if `[glossary].enabled` is false; `--no-glossary` forbids it for this run. Glossary build failure is non-fatal (reported under `glossary.error`).

### `status --workspace <dir>`

`{"engine","game_path","source_lang","target_lang","total","translated","pending","passthrough","domains":{…}}`. `passthrough > 0` → consider `translate --retry-passthrough`.

### `review --workspace <dir>`

Mechanical post-translate scan (no LLM). JSON: `total` / `translated` / `pending` / `passthrough`, plus `glossary` (same shape as `glossary check`) and buckets `residual_source`, `identical`, `control_loss`, `namebox_mismatch` (`count` + `sample` up to 40). `attx run` attaches this after translate.

### `preserve`

| Subcommand | Role |
|------------|------|
| `list --workspace <dir>` | Builtin + workspace regexes (JSON) |
| `add --workspace --pattern <re> [--info]` | Append a workspace rule; hits become `[CTRL_n]` |
| `remove --workspace --pattern <re>` | Drop a workspace rule by exact pattern |

Builtins always apply: RMMZ control codes, `{ident}`, `%s`/`%d`. Engine `renpy` also keeps `[ident]`. Workspace file: `preserve.toml`.

### JSONL interchange

| Command | Role |
|---------|------|
| `translate-jsonl --input --output [--src] [--dst] [--limit]` | No workspace: `{id,text,context?,role?,item_type?}` in, `translation`+`translation_lines` added out |
| `export-jsonl --workspace --output [--filter pending\|all\|translated\|passthrough]` | Workspace → JSONL |
| `import-jsonl --workspace --input` | JSONL → workspace (matches by `id`) |

### `learn`

| Subcommand | Role |
|------------|------|
| `summarize --workspace <dir> [--llm]` | Turn run evidence into experience entries (`scan` is an alias); `--llm` adds model review (costs money) |
| `pending` | Entries awaiting approval, with evidence (JSON) |
| `review --approve 1,3 [--reject 2] [--approve-all]` | Approve/reject by 1-based index |
| `list [--format <id>]` | Active entries (JSON) |
| `defaults --format <id>` | Print the built-in baseline for a format (TOML) |
| `forget --field <name> [--format <id>]` | Drop entries by field name |

### `glossary`

| Subcommand | Role |
|------------|------|
| `build --workspace <dir> [--method llm\|stats] [--min-occurrences N] [--dry-run]` | Extract proper nouns into `glossary.toml`; always dry-run first |
| `list [--all]` | Terms as JSON (`--all` includes model-rejected ones) |
| `add --src <term> --dst <translation> [--info <desc>]` | Add/overwrite one term |
| `remove --src <term>` | Remove one term |
| `import --file <json>` / `export --file <json>` | `[{src,dst,info}]` or `{src: dst}` |
| `check` | Terms the translation did not actually use (violations, by count) |
