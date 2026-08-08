# 使い方

```bash
attx run --input book.epub --src ja --dst zh
```

段階実行：`detect` → `init` → `extract` → `translate --limit 20` → `translate` → `writeback`。

JSONL レビュー・用語集・learn は README を参照。
