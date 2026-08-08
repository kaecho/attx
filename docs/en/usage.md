# Usage

## One-shot

```bash
attx run --input book.epub --src ja --dst zh
# → book.zh.epub next to the input
```

## Step-by-step

```bash
attx detect  --input book.epub
attx init    --input book.epub --src ja --dst zh
attx extract --workspace .attx-book
attx status  --workspace .attx-book
attx translate --workspace .attx-book --limit 20
attx translate --workspace .attx-book
attx writeback --workspace .attx-book
```

Most formats write a **sibling copy**. Directory adapters that overwrite in place create `*.attxbak` — use `--dry-run` first.

## Offline / human review (JSONL)

```bash
attx export-jsonl --workspace .attx-book --output pending.jsonl --filter pending
attx import-jsonl --workspace .attx-book --input pending.jsonl
attx writeback --workspace .attx-book
```

No workspace:

```bash
attx translate-jsonl --input source.jsonl --output out.jsonl --src ja --dst zh
```

## Glossary (optional)

```toml
[glossary]
enabled = true
min_occurrences = 10
max_terms = 200
inject_limit = 30
```

```bash
attx glossary build --workspace .attx-book --dry-run
attx glossary build --workspace .attx-book
attx glossary list  --workspace .attx-book
attx glossary check --workspace .attx-book
```

## Experience / learn

After writeback, attx can record skip/extract hints (no API cost by default):

```bash
attx learn pending
attx learn review --approve 1,3
attx learn list
attx writeback --workspace .attx --no-learn   # skip capture once
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
