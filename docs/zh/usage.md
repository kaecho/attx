# 用法

## 一键运行

```bash
attx run --input book.epub --src ja --dst zh
# → book.zh.epub next to the input; the original is never touched
```

`run` = `init` → `extract` →（可选术语表）→ `translate` → `writeback`，并打印每个阶段的 JSON 报告。常用标志：

| 标志 | 效果 |
|------|------|
| `--limit 20` | 试译：最多翻译 20 个单元 |
| `--no-translate` | 只提取，然后打印状态 |
| `--no-writeback` | 在翻译后停止（写回前检查） |
| `--glossary` / `--no-glossary` | 覆盖本次运行的 `[glossary].enabled` |

## 分步执行（大型输入）

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

### 工作区

目录输入使用 `<dir>/.attx`；文件输入使用 `<parent>/.attx-<stem>`。

| 文件 | 作用 |
|------|------|
| `attx.db` | SQLite：单元、译文、工作区元数据 |
| `workspace.json` | 工作区元数据的可读快照 |
| `glossary.toml` | 术语表构建后的术语 |
| `experience.toml` | 本工作区的 skip 字段规则和 `topic=prompt` 文风 note |
| `profile.toml` | 使用自定义 Profile 时它的副本 |

### 阅读 `status`

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

- `pending` = 没有有效译文（或源文已变化）的单元。`translate` 只处理这些。
- `passthrough` = 因模型拒答或反复失败而以未改动原文作为“译文”的单元。它们被计为已翻译以便运行完成 —— 用 `translate --retry-passthrough` 重新入队。

## 输出约定

- **文件类格式写出翻译旁路副本** `<name>.<dst>.<ext>`；源文件永不修改。
- **`rmmz`（RPG Maker）原地写回**游戏目录（`data/*.json`、`js/plugins.js`），每个被覆盖的文件都会做一次 `*.attxbak` 备份。
- **`jsonl` 目录模式**在 `source.jsonl` 旁边写出 `translated.jsonl`。
- `overwrite = true` 的自定义 Profile 也会原地写回。

在让任何运行覆盖内容之前，务必先 `writeback --dry-run` 并检查 `paths[]`。

## 手动 / 离线审校（JSONL）

导出待译单元，手工编辑（或交给人类审校），导入，写回：

```bash
attx export-jsonl --workspace .attx-book --output pending.jsonl --filter pending
attx import-jsonl --workspace .attx-book --input pending.jsonl
attx writeback    --workspace .attx-book
```

过滤器：`pending`（默认）| `all` | `translated` | `passthrough`。导入按 `id` 匹配单元，需要非空的 `translation_lines` 或 `translation`。

独立使用，完全不需要工作区：

```bash
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh
```

输入行：`{"id","text","context"?,"role"?,"item_type"?}` → 输出行增加 `translation` 与 `translation_lines`。

## 未知输入？发现工具链

```bash
attx analyze --input ./project              # recon report (JSON)
attx profile new --output fmt.toml          # commented rule template
attx profile test --profile fmt.toml --input ./project --roundtrip
attx init --input ./project --profile fmt.toml --src ja --dst zh
attx profile save --profile fmt.toml        # detect recognizes it from now on
```

完整流程：[格式](formats.md) → *未知格式？教 attx 一个 Profile*。

## 术语表（可选）

让整部作品的专有名词译名保持一致。默认关闭 —— 构建术语表需要额外的 LLM 调用。

```bash
attx glossary build --workspace .attx-book --dry-run   # size the run, spend nothing
attx glossary build --workspace .attx-book
attx glossary list  --workspace .attx-book
attx glossary check --workspace .attx-book             # terms the translation ignored
```

LLM 提取策略与 `[glossary]` 配置项见 README 的 *术语表* 一节（`min_occurrences` 默认 10）。

## 经验 / 学习

写回后，attx 会自动记录 skip/extract 提示（默认不消耗 API 费用）：

```bash
attx learn pending                       # entries awaiting approval, with evidence
attx learn review --approve 1,3          # approve — only now can they delete anything
attx learn list                          # active entries
attx writeback --workspace .attx --no-learn   # skip capture once
attx extract --no-knowledge              # ignore all learned rules
```

## CLI 一览

| 命令 | 作用 |
|---------|------|
| `doctor [--ping]` | 配置 / LLM 连通性检查 |
| `formats` / `detect` / `analyze` | 格式 |
| `profile …` | 自定义 Profile |
| `init` / `extract` / `translate` / `writeback` / `run` | 流水线 |
| `status` / `export-jsonl` / `import-jsonl` | 进度与交换 |
| `learn …` / `glossary …` | 经验与术语 |

带标志的完整参考：[CLI](cli.md)。
