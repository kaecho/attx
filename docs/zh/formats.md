# 格式与未知输入

`attx formats` 是当前二进制的格式清单。自动选择顺序为专用内置适配器、已保存 Profile、`auto` 内容探测。相同 `.json` 扩展名按结构选择，不能仅凭扩展名判断用途。强制选择用 `init --engine` 或 `run --engine`，`detect` 没有这个选项。

## 内置适配器

| id | 输入 | 提取与输出范围 |
|----|------|----------------|
| `rmmz` | RPG Maker MV/MZ 目录 | 数据库、事件对白、姓名框、选项、滚动文字和安全插件参数；原地写回，见[专页](rmmz.md) |
| `epub` | `.epub` | 叶子正文块如段落、标题、列表项；跳过 ruby 注音，保留图片和结构，更新语言元数据；语言后缀副本 |
| `html` | `.html`、`.htm`、`.xhtml` | HTML 正文块；语言后缀副本 |
| `docx` | `.docx` | 正文及脚注/尾注的段落文本，译文由段落首个 run 承载；副本，不保证原来逐 run 的字体分配 |
| `xlsx` | `.xlsx`、`.xlsm` | 共享字符串 `xl/sharedStrings.xml`，跳过注音 run；副本，不是公式、图表、所有 inline 字符串或宏的翻译器 |
| `srt` | `.srt` | 字幕正文，时间和序号保留；副本 |
| `vtt` | `.vtt` | 字幕正文，头部和时间信息保留；副本 |
| `ass` | `.ass`、`.ssa` | `Dialogue:` Text，保护覆盖标签和 `\N`，Name 可作角色；副本 |
| `lrc` | `.lrc` | 歌词正文，时间标签和元数据保留；副本 |
| `csv` | `.csv`、`.tsv` | 按单元格提取，支持引号字段及字段内换行，只重写含源文的记录；副本 |
| `po` | `.po`、`.pot` | 填入 `msgstr`，头部和 plural 条目直通；副本，不是完整 gettext 复数翻译器 |
| `renpy` | `.rpy` | `translate` 块内对白及 `old`/`new` 对，跳过资源语句；副本，不解析所有 Ren'Py/Python 程序 |
| `md` | `.md`、`.markdown` | 逐行提取正文并保留标题/列表前缀，跳过 fenced code；不是完整 Markdown AST，副本 |
| `txt` | `.txt` | 逐行正文单元；副本 |
| `paratranz` | `.json`，内容嗅探 | Paratranz 导出中待填的 translation；副本 |
| `vnt` | `.json`，内容嗅探 | VNTextPatch 的 name/message 数据；副本 |
| `mtool` | `.json`，内容嗅探 | MTool 翻译映射，如 ManualTransFile；副本 |
| `i18next` | `.json`，内容嗅探 | 嵌套字符串叶子；副本 |
| `jsonl` | `.jsonl` 或含 `source.jsonl` 的目录 | 不按源语言过滤输入记录；文件输出语言后缀副本，目录输出 `translated.jsonl` |
| `auto` | 文件或目录，按内容 | 下述保守文本子集；文件副本或目录 `translated-<dst>/` |

`custom:<name>` 是 Profile 动态适配器，不是固定内置格式。一般文档副本为 `<stem>.<dst>.<ext>`，实际以 writeback 的 `paths` 为准。已有目标也会备份，而非任意覆盖。

## `auto` 支持什么

未知后缀不代表不可翻译。`auto` 尝试解码并识别可信结构，只替换明确的人类文本跨度。

- JSON：在任意层次识别字符串值，避开 key、标识符、资源路径等机器数据；保留未改跨度的字节、空白和布局。嵌套扫描深度超过 128 会拒绝，而不是递归处理任意深度。
- XML：处理严格 XML 1.0 字符数据，转义译文并保留标签/属性/注释；不翻译 CDATA，也不支持 DTD、实体声明或 XML 1.1。
- INI、TOML、简单 YAML：只支持可可靠定位的标量子集。不是完整 YAML parser，不承诺多行、复杂 flow、anchor 或所有转义写法。
- 正文：只有内容足够像自然语言时才接管，包括没有已知扩展名的纯文本。不把脚本代码、二进制或含混结构猜成正文。

机器文字和源语言判断都是启发式，可能漏掉 UI 单词或专名，也可能选到语义上的标识。查看提取样本及覆盖报告；不能把结构可往返理解为所有字段都选对了。

## 混合目录的边界

```bash
attx detect --input './project'
attx run --input './project' --engine auto --src ja --dst zh --no-infer
```

目录 `auto` 会把支持的文本写到 `project/translated-zh/` 对应路径，不支持的候选文件原样复制。它不在目录里再次分派 EPUB、字幕或脚本专用适配器；例如目录中的 EPUB 不会自动拆包翻译。需要其专用行为时逐个以文件运行，或使用明确的外部批处理。

`extract.auto_coverage` 含：

| 字段 | 含义 |
|------|------|
| `supported_files` | 有可处理文本跨度的文件数量，不是成功译文数量 |
| `copied_files` | 未支持、会原样复制的候选文件数量 |
| `unsupported_total` | 不支持的候选文件总数 |
| `unsupported_paths` | 最多 50 个路径样例，不是完整清单 |
| `excluded_entries` | 未遍历的生成目录、元数据、备份和符号链接项数量；被排除目录根计1，不统计内部所有文件 |
| `excluded_paths` | 最多50个排除路径样例，与 unsupported_paths 分开 |

`.attx*`、`.git`、`node_modules`、生成目录 `translated-*/translated`、backup/backups 和备份后缀被排除。含语言后缀的源资源有歧义时原样复制，不静默省略。符号链接不遍历，输入或输出路径中的不安全链接会被拒绝。排除项不是复制覆盖范围的一部分；它们单独计入 excluded_entries/excluded_paths。若全部候选没有提取单元，`run` 会停止，不能把原样复制说成翻译成功。

## 编码

通用文本读取先尝试严格 UTF-8、带 BOM 的 UTF-16，再以 chardetng/encoding_rs 猜测旧编码。专用文本适配器和 Profile 通常输出 UTF-8，不能笼统承诺保留 Shift-JIS/GBK；DOCX、EPUB 等容器按其自身格式处理。

`auto` 要求源文无损解码和旧编码往返，保留原编码及 BOM。译文无法用原编码表示时失败，例如某些中文无法写入 Shift-JIS。正确办法是在外部工具中制作 UTF-8 输入副本并用新工作区，不能用替换字符吞掉文字或改 hash。

## 未识别时如何继续

1. `analyze --input <路径>` 查看编码、结构、二进制容器线索和样本。
2. 简单行结构或 JSON 可用[Profile](profiles.md)。`run` 在允许翻译、没有匹配适配器时可自动请求模型推断；`--no-infer` 禁止这项额外费用。
3. 脚本转义、二进制、加密/压缩的未知容器或模糊语法，应使用可靠外部提取器导出 JSONL，并由外部工具写回。

PDF、OCR、任意游戏引擎的二进制脚本和运行时字体修补不在当前内置覆盖中。不要承诺“任意格式”都能直接原地翻译。

继续：[Profile](profiles.md) · [JSONL 工作流](usage.md) · [故障排查](troubleshooting.md)。
