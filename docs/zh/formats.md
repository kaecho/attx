# 格式

## 内置适配器

`attx formats` 以 JSON 打印权威清单 —— id、扩展名，以及输入是文件还是目录。检测顺序固定；`.json` 各变体按内容嗅探，最具体的优先。

| id | 扩展名 | 输入 | 输出 |
|----|-----------|-------|--------|
| `rmmz` | — | 目录 | 原地 + `*.attxbak` |
| `epub` | `.epub` | 文件 | `<name>.<dst>.epub` |
| `html` | `.html` `.htm` `.xhtml` | 文件 | 翻译副本 |
| `docx` | `.docx` | 文件 | `<name>.<dst>.docx` |
| `xlsx` | `.xlsx` `.xlsm` | 文件 | 翻译副本 |
| `srt` | `.srt` | 文件 | 翻译副本 |
| `vtt` | `.vtt` | 文件 | 翻译副本 |
| `ass` | `.ass` `.ssa` | 文件 | 翻译副本 |
| `lrc` | `.lrc` | 文件 | 翻译副本 |
| `csv` | `.csv` `.tsv` | 文件 | 翻译副本 |
| `po` | `.po` `.pot` | 文件 | 翻译副本 |
| `renpy` | `.rpy` | 文件 | 翻译副本 |
| `md` | `.md` `.markdown` | 文件 | 翻译副本 |
| `txt` | `.txt` | 文件 | 翻译副本 |
| `paratranz` | `.json`（嗅探） | 文件 | 翻译副本 |
| `vnt` | `.json`（嗅探） | 文件 | 翻译副本 |
| `mtool` | `.json`（嗅探） | 文件 | 翻译副本 |
| `i18next` | `.json`（嗅探） | 文件 | 翻译副本 |
| `jsonl` | `.jsonl`，或含 `source.jsonl` 的目录 | 文件或目录 | `translated.jsonl` |
| `custom:<name>` | 来自 Profile | 文件或目录 | 副本，`overwrite = true` 时原地写回 |

当检测有歧义或错误时强制指定适配器：`attx init --engine <id>`。

### 各格式说明

- **epub** —— 段落级单元，覆盖叶子块（`p`、标题、`li`、……）；注音假名（`<rt>`/`<rp>`）会从源文中剔除；图片与排版保留；写回时更新 `dc:language`。
- **docx** —— 段落级，覆盖 `w:t` run（正文 + 脚注/尾注）；每段第一个 run 接收译文。
- **xlsx** —— 翻译共享字符串表（`xl/sharedStrings.xml`），因此所有工作表保持一致；注音 `rPh` run 跳过。
- **srt/vtt/lrc** —— 时间轴行、头部与元数据原样保留；只翻译字幕/歌词文本。
- **ass** —— 只翻译 `Dialogue:` 的 Text 字段；`{\tag}` 覆盖与 `\N` 换行保留；`Name` 列作为说话人角色。
- **csv/tsv** —— 按单元格的单元（RFC 4180：带引号字段、内嵌换行）；只重写含源语言文本的记录。
- **po** —— 填充 `msgstr`；头部条目与 `msgid_plural` 条目原样直通。
- **renpy** —— 只在 `translate` 块内：带引号的对白以及 `old`/`new` 字符串对；资源语句（voice/play/show/……）跳过。
- **rmmz** —— 见 [RPG Maker](rmmz.md)。
- **mtool/paratranz/vnt/i18next** —— 按内容嗅探的 JSON 形态（MTool `ManualTransFile.json`、Paratranz 导出只填空的 `translation` 字段、VNTextPatch `name`/`message`、i18next 嵌套字符串叶子）。
- **jsonl** —— 逃生舱：通过外部提取/写回脚本支持任意引擎；提取时不按源语言过滤。

### 编码

文本输入自动检测编码：严格 UTF-8 → UTF-16（BOM）→ `chardetng` 猜测（Shift-JIS、GBK、……）→ `encoding_rs` 解码。输出**一律 UTF-8**。

## 未知格式？教 attx 一个 Profile

```bash
attx analyze --input ./project         # recon: encoding, structure, samples, JSON shape
attx profile new --output fmt.toml     # documented rule template
attx profile test --profile fmt.toml --input ./project --roundtrip   # iterate
attx init --input ./project --profile fmt.toml --src ja --dst zh
attx profile save --profile fmt.toml   # detect auto-recognizes it from now on
```

### Profile 结构

```toml
name = "myformat"                    # id → engine "custom:myformat"
label = "My format"
extensions = ["ks"]                  # e.g. ["ks", "scn"]
detect_regex = []                    # ALL must match in the first 64 KiB
min_units = 1                        # auto-detect needs ≥ this many units
overwrite = false                    # true = write back in place
skip_lines = []                      # line_regex mode: skip matching lines
notes = ""

# Per-line regex: (?P<text>...) required, (?P<role>...) optional
[[rules]]
kind = "line_regex"
pattern = '^(?P<role>[^\s@;]*)\s*「(?P<text>.+)」$'

# JSON: string values under these object keys (any depth)
[[rules]]
kind = "json_keys"
keys = ["message", "name"]

# JSON: string leaves at path globs (* one level, ** any depth)
[[rules]]
kind = "json_paths"
paths = ["events/*/text", "**/choices/*"]
```

已保存的 Profile 位于 `$ATTX_HOME/profiles/`（或 `~/.config/attx/profiles/`），并以 `custom:<name>` 出现在 `attx formats` / `attx detect` 中。

示例：`profiles/examples/`（KiriKiri KAG、INI、通用 JSON）。Agent 流程：`skills/attx/references/custom-format-discovery.md`。
