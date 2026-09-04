# Usage

## One-shot

```bash
attx run --input book.epub --src ja --dst zh
# → book.zh.epub next to the input; the original is never touched
```

`run` = `init` → `extract` → (optional glossary) → `translate` → `writeback`, and prints a JSON report of every stage. Useful flags:

| Flag | Effect |
|------|--------|
| `--limit 20` | Trial: translate at most 20 units |
| `--no-translate` | Extract only, then print status |
| `--no-writeback` | Stop after translate (inspect before writing) |
| `--glossary` / `--no-glossary` | Override `[glossary].enabled` for this run |

## Step-by-step (large inputs)

```bash
attx detect  --input book.epub
attx init    --input book.epub --src ja --dst zh      # workspace: .attx-book/
attx extract --workspace .attx-book
attx status  --workspace .attx-book
attx translate --workspace .attx-book --limit 20      # trial first
attx translate --workspace .attx-book                 # full; re-run to resume
attx writeback --workspace .attx-book --dry-run       # preview planned files
attx writeback --workspace .attx-book                 # → book.zh.epub
```

### The workspace

A directory input gets `<dir>/.attx`; a file input gets `<parent>/.attx-<stem>`.

| File | Role |
|------|------|
| `attx.db` | SQLite: units, translations, workspace meta |
| `workspace.json` | Readable snapshot of the workspace meta |
| `glossary.toml` | Terms, when a glossary was built |
| `experience.toml` | Learned skip-fields and `topic=prompt` notes for this workspace |
| `profile.toml` | Copy of the custom profile, when one was used |

### Reading `status`

```json
{
  "engine": "txt",
  "game_path": "/path/book.txt",
  "source_lang": "ja",
  "target_lang": "zh",
  "total": 1000,
  "translated": 20,
  "pending": 980,
  "passthrough": 0,
  "domains": { "text": { "total": 1000, "translated": 20 } }
}
```

- `pending` = units with no valid translation (or whose source changed). `translate` only touches these.
- `passthrough` = units whose "translation" is the untouched original because the model refused or kept failing. They count as translated so the run can finish — re-queue with `translate --retry-passthrough`.

## Output conventions

- **File formats write a translated sibling** `<name>.<dst>.<ext>`; the source file is never modified.
- **`rmmz` (RPG Maker) writes in place** into the game directory (`data/*.json`, `js/plugins.js`), with a one-time `*.attxbak` backup of each overwritten file.
- **`jsonl` directory mode** writes `translated.jsonl` next to `source.jsonl`.
- Custom profiles with `overwrite = true` also write in place.

Always `writeback --dry-run` first and review `paths[]` before letting a run overwrite anything.

## Manual / offline review (JSONL)

Export pending units, edit them by hand (or have a human review), import, write back:

```bash
attx export-jsonl --workspace .attx-book --output pending.jsonl --filter pending
attx import-jsonl --workspace .attx-book --input pending.jsonl
attx writeback    --workspace .attx-book
```

Filters: `pending` (default) | `all` | `translated` | `passthrough`. Import matches units by `id` and needs a non-empty `translation_lines` or `translation`.

Standalone, no workspace at all:

```bash
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh
```

Input lines: `{"id","text","context"?,"role"?,"item_type"?}` → output lines add `translation` + `translation_lines`.

## Unknown input? The discovery toolchain

```bash
attx analyze --input ./project              # recon report (JSON)
attx profile new --output fmt.toml          # commented rule template
attx profile test --profile fmt.toml --input ./project --roundtrip
attx init --input ./project --profile fmt.toml --src ja --dst zh
attx profile save --profile fmt.toml        # detect recognizes it from now on
```

Full workflow: [Formats](formats.md) → *Unknown format? Teach attx a profile*.

## Glossary (optional)

Consistent proper-noun names across a whole work. Off by default — building one costs extra LLM calls.

```bash
attx glossary build --workspace .attx-book --dry-run   # size the run, spend nothing
attx glossary build --workspace .attx-book
attx glossary list  --workspace .attx-book
attx glossary check --workspace .attx-book             # terms the translation ignored
```

See the README's *Glossary* section for the LLM extraction strategy and the `[glossary]` config keys (`min_occurrences` defaults to 10).

## Experience / learn

After writeback, attx records skip/extract hints automatically (no API cost by default):

```bash
attx learn pending                       # entries awaiting approval, with evidence
attx learn review --approve 1,3          # approve — only now can they delete anything
attx learn list                          # active entries
attx writeback --workspace .attx --no-learn   # skip capture once
attx extract --no-knowledge              # ignore all learned rules
```

## CLI map

| Command | Role |
|---------|------|
| `doctor [--ping]` | Config / LLM ping |
| `formats` / `detect` / `analyze` | Formats |
| `profile …` | Custom profiles |
| `init` / `extract` / `translate` / `writeback` / `run` | Pipeline |
| `status` / `export-jsonl` / `import-jsonl` | Progress & interchange |
| `learn …` / `glossary …` | Experience & terms |

Full reference with flags: [CLI](cli.md).
