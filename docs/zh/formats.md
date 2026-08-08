# 格式

| id | 输入 | 输出 |
|----|------|------|
| `rmmz` | 游戏目录 | 原地 + `*.attxbak` |
| `epub` | `.epub` | `<name>.<dst>.epub` |
| `html` / `docx` / `xlsx` | 文件 | 译出副本 |
| `txt` / `md` | 文本 | `<name>.<dst>.*` |
| `srt` / `vtt` / `ass` / `lrc` | 字幕 | 译出副本 |
| `csv` / `po` / `renpy` | 文件 | 译出副本 |
| `mtool` / `paratranz` / `vnt` / `i18next` | `.json` | 译出副本 |
| `jsonl` / `custom:<name>` | 文件/目录 | 通用 / profile |

```bash
attx formats
attx detect --input <path>
```

## 未知格式

```bash
attx analyze --input ./game
attx profile new --output fmt.toml
attx profile test --profile fmt.toml --input ./game --roundtrip
attx init --input ./game --profile fmt.toml --src ja --dst zh
attx profile save --profile fmt.toml
```

详见 `profiles/examples/` 与 `skills/attx/references/custom-format-discovery.md`。
