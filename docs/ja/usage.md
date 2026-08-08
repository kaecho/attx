# 使い方

## 一括

```bash
attx run --input book.epub --src ja --dst zh
```

## 段階実行

```bash
attx detect  --input book.epub
attx init    --input book.epub --src ja --dst zh
attx extract --workspace .attx-book
attx status  --workspace .attx-book
attx translate --workspace .attx-book --limit 20
attx translate --workspace .attx-book
attx writeback --workspace .attx-book
```

## JSONL レビュー

```bash
attx export-jsonl --workspace .attx-book --output pending.jsonl --filter pending
attx import-jsonl --workspace .attx-book --input pending.jsonl
attx writeback --workspace .attx-book
```

## 用語集 / 学習

```bash
attx glossary build --workspace .attx-book
attx learn pending
attx learn review --approve 1,3
```
