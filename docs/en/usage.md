# Usage

## One-shot

```bash
attx run --input book.epub --src ja --dst zh
# → book.zh.epub next to the input
```

## Step-by-step

```bash
attx detect  --input book.epub
attx init    --input book.epub --src ja --dst zh      # workspace .attx-book/
attx extract --workspace .attx-book
attx status  --workspace .attx-book
attx translate --workspace .attx-book --limit 20      # trial
attx translate --workspace .attx-book                 # full; re-run resumes
attx writeback --workspace .attx-book
```

## Offline / human review (JSONL)

```bash
attx export-jsonl --workspace .attx-book --output pending.jsonl --filter pending
# edit translation / translation_lines
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
attx glossary build --workspace .attx-book
attx glossary list  --workspace .attx-book
```

## Experience / learn

After writeback, attx can record skip/extract hints (no API cost by default):

```bash
attx learn pending
attx learn review --approve 1,3
attx learn list
```

Use `writeback --no-learn` to skip capture for one run.
