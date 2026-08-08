# 用法

## 一键

```bash
attx run --input book.epub --src ja --dst zh
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

多数格式写**旁路副本**。会原地覆盖的适配器带 `*.attxbak`——先 `--dry-run`。

## JSONL 审校 / 术语表 / 经验层

```bash
attx export-jsonl --workspace .attx-book --output pending.jsonl --filter pending
attx glossary build --workspace .attx-book --dry-run
attx learn pending
```

详见仓库 README 中「自我改进的经验层」「术语表」两节（与 0.6 起能力一致）。
