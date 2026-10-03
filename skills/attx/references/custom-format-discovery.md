# 未知文本格式：探测、验证和复用

优先专用适配器，再尝试保存的 Profile，最后由 `auto` 保守嗅探内容。不要仅因扩展名陌生就放弃，也不要为了“支持任何格式”把脚本和二进制当成正文。

## 先用本地探测

```bash
attx detect --input <输入>
attx analyze --input <输入> --src ja
```

读取 `engine`、`content_root`、`profile`。`analyze` 提供编码、结构、样本、源语言密度，目录还提供扩展名分布与抽样。`detect` 没有 `--engine`；需要强制已知引擎时用 `run` 或 `init`。

`auto` 根据内容处理可识别的 JSON、XML、标量配置和正文，不依赖固定扩展名。它只翻译可靠定位的文本，保留非正文结构。目录中不支持的文件可能原样复制，必须报告覆盖情况，不能说成全部内容已翻译。

提取报告的 `auto_coverage` 含 `supported_files`、`copied_files`、`unsupported_total`、`unsupported_paths`（最多 50 个样例）。这是覆盖报告，不是成功翻译数量，也不是所有跳过文件的完整清单。

```bash
attx run --input <输入> --engine auto --src ja --dst zh
```

只有探测证据支持时才强制 `auto`。源文编码、结构和文件身份校验失败时停止，不扩大规则来掩盖问题。

## 有限模型推断

`run` 没有匹配适配器时可自动推断 Profile；`--no-infer` 可关闭这个模型步骤。自动推断有额外请求，但属于默认格式处理流程，不等于自动开启付费 glossary。

也可独立推断，先查看结果：

```bash
attx profile infer --input <输入> --output ./fmt.toml --src ja --name myformat
```

固定最多 3 次提案和验证尝试。输出已存在时拒绝覆盖。每个提案只含声明式规则，不执行代码；强制 `overwrite=false`，经过试提取及源文无损/no-op 往返验证。成功报告 `profile`、`engine`、`units`、`attempts`、`output`、`roundtrip`、`overwrite` 和 `status`。

失败时说明无法安全处理的具体结构，不能反复启动推断或执行模型建议的任意脚本。二进制、加密、偏移表封包需要已有外部提取器或用户提供的 JSONL，见 `jsonl-workflow.md`。

## 手工声明式 Profile

本地样本足以确定规则时可以创建并编辑任务 Profile，不必调用模型：

```bash
attx profile new --output ./fmt.toml --name myformat
```

| kind | 用途 | 字段 |
|------|------|------|
| `line_regex` | 单行脚本或配置中的明确文本片段 | `pattern`，必须有 `(?P<text>...)`，可选 `(?P<role>...)` |
| `json_keys` | 任意深度的固定正文字段 | `keys = ["message", "description"]` |
| `json_paths` | 精确定位 JSON 正文 | `paths`；`*` 一层，`**` 任意层 |

规则可混用，JSON 规则仅在内容可解析为 JSON 时生效。`extensions` 用于目录扫描，`detect_regex` 限定识别特征，`min_units` 要求最低提取量，`skip_lines` 排除注释或命令，`notes` 说明规则依据。样例在 `profiles/examples/`。

任务新 Profile 保持 `overwrite=false`，默认产出翻译副本。用户明确选择的既有 `overwrite=true` Profile 可以正常原地写回并保留备份，无需再问写回许可；不能把推断结果自行改成 true。

## 验证和正式执行

```bash
attx profile test --profile ./fmt.toml --input <输入> --src ja --roundtrip
```

读取：

- `units` 是否符合正文范围，`sample[].text` 是否混入代码、路径、标签或资源标识。
- `sample[].role` 是否是实际角色，不要把命令参数当角色。
- `detects` 是否为 true，避免一个过宽 Profile 误接管别的格式。
- `roundtrip.ok` 是否为 true。往返在内存完成，不等于实际生成译文文件，也不能证明真实译文质量。

根据证据修正规则，给本次迭代设有限次数；始终不能靠放宽过滤来凑数量。无法确认源文无损和正文边界时停止。

```bash
attx run --input <输入> --profile ./fmt.toml --src ja --dst zh
```

Profile 拷贝为 `<工作区>/profile.toml`，工作区可复现。正常翻译请求包括此输入的写回，规模较大也不默认要求再次批准全量。用户要先试译时加 `--limit N --no-writeback`；只在接受剩余问题时加 `--allow-partial`。

## 保存和复用

已验证并成功用于任务的 Profile 可以保存供后续匹配；用户要求记住格式时直接执行，不再问同样的问题。

```bash
attx profile list
attx profile save --profile ./fmt.toml
```

保存到 `$ATTX_HOME/profiles/`，否则使用用户配置目录中的 `attx/profiles/`。后续 `detect`、`formats`、`run --engine custom:<名称>` 可复用。

先检查已存名称。相同 Profile 直接复用，不制造重复项；同名不同内容保留原项，给新 Profile 明确的新名称。不要默默 `--force` 覆盖其他任务的规则。确需替换既有规则时说明差异并取得针对该替换的许可。

编码依适配器处理。不要笼统承诺所有输出保留原编码或都能直接用于只支持 Shift-JIS 的引擎；明确说明报告中的编码约束，无法表示译文时停止，而不是用替换字符写坏文本。
