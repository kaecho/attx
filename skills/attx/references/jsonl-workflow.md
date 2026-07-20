# 通用 JSONL 流程

用于：非 RM 引擎、外部提取器、人工审校、跨工具对接（对应 att-mz issue #11 思路）。

## 行格式

输入：

```json
{"id":"scene1:55","text":"改札でごった返す人混みの中、…","context":"00_op_000","role":"ヒロイン","item_type":"long_text"}
```

| 字段 | 必填 | 说明 |
|------|------|------|
| id | 是 | 稳定定位键，写回时你的脚本用它 |
| text | 是 | 原文；多行用 `\n` |
| context | 否 | 同场景分批 |
| role | 否 | 说话人 |
| item_type | 否 | `long_text` / `array` / `short_text` |

输出额外：

```json
{"translation":"…","translation_lines":["…"]}
```

## 无工作区直译

```bash
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh
```

外部：

```bash
extract_game.sh > source.jsonl
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja
write_game.sh translated.jsonl
```

## 经工作区（可写回 rmmz 已提取单元）

```bash
attx export-jsonl --workspace <WS> --output pending.jsonl --filter pending
# 审校 / 外翻
attx import-jsonl --workspace <WS> --input translated.jsonl
attx writeback --workspace <WS>
```

## jsonl 适配器目录模式

目录内放 `source.jsonl`：

```bash
attx init --game <含source.jsonl的目录> --engine jsonl --src ja --dst zh
attx extract --workspace ...
attx translate ...
attx writeback ...   # 写出 translated.jsonl
```
