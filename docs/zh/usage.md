# 用法

## 一键

```bash
attx run --input book.epub --src ja --dst zh
# → 旁路生成 book.zh.epub
```

## 分步

```bash
attx detect  --input book.epub
attx init    --input book.epub --src ja --dst zh
attx extract --workspace .attx-book
attx status  --workspace .attx-book
attx translate --workspace .attx-book --limit 20
attx translate --workspace .attx-book
attx writeback --workspace .attx-book
```

## 离线校对（JSONL）

```bash
attx export-jsonl --workspace .attx-book --output pending.jsonl --filter pending
# 编辑 translation / translation_lines
attx import-jsonl --workspace .attx-book --input pending.jsonl
attx writeback --workspace .attx-book
```

无工作区：

```bash
attx translate-jsonl --input source.jsonl --output out.jsonl --src ja --dst zh
```

## 术语表（可选）

```toml
[glossary]
enabled = true
```

```bash
attx glossary build --workspace .attx-book
attx glossary list  --workspace .attx-book
```

## 经验层 learn

写回后可自动记录 skip/extract 提示（默认无 API 费用）：

```bash
attx learn pending
attx learn review --approve 1,3
attx learn list
```

单次跳过：`writeback --no-learn`。
