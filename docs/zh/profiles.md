# 自定义 Profile

Profile 是小型声明式 TOML 格式规则，不是模型生成的程序。它指定哪段文本可提取、位置如何对应，以及写副本还是原地写回。已保存的规则以 `custom:<name>` 参与检测；工作区会保存不可变的规则快照。

## 手工定义并测试

```bash
attx analyze --input './script.ks' --src ja
attx profile new --output './kag-local.toml' --name kag-local
# 编辑生成的 TOML
attx profile test --profile './kag-local.toml' --input './script.ks' --src ja --limit 10 --roundtrip
attx run --input './script.ks' --profile './kag-local.toml' --src ja --dst zh
attx profile save --profile './kag-local.toml'
```

`new` 不覆盖已有输出。`test --roundtrip` 在内存中用标记译文调用写回，不生成真实翻译文件；检查 `roundtrip.ok`，不能只看命令返回 0。`save` 同名存在时需要明确的 `--force`，通常应给不同规则新的名字。

[示例目录](https://github.com/kaecho/attx/tree/main/profiles/examples)含 KiriKiri KAG、INI 和通用 JSON 规则。它们不是所有版本和脚本语法都经过验证的通用插件。

## 完整键参考

| 键 | 类型 | 默认 | 含义 |
|----|------|------|------|
| `name` | 字符串 | 必填 | 非空 ASCII 字母、数字、`_`、`-`；引擎为 `custom:<name>` |
| `label` | 字符串 | 空 | 可读名称，空时用默认描述 |
| `extensions` | 字符串数组 | `[]` | 小写后缀，不含点；目录输入需要指定 |
| `detect_regex` | 正则字符串数组 | `[]` | 检测时每个表达式都须在前 64 KiB 文本内命中 |
| `min_units` | 非负整数 | `1` | 自动检测至少提取这么多单元 |
| `overwrite` | 布尔 | `false` | false 写语言后缀副本；true 原地写回并备份 |
| `skip_lines` | 正则字符串数组 | `[]` | 行模式中在匹配规则前排除行 |
| `notes` | 字符串 | 空 | 给维护者的解释，不自动作为翻译风格 prompt |
| `rules` | 规则数组 | 必填且非空 | 下述一种或多种规则 |

未知字段被拒绝。规则正则使用 Rust regex 语法，不支持任意 PCRE 回溯、lookaround 或 backreference。不要把捕获的语法标记包含进可替换 text。

### `line_regex`

```toml
name = "quoted-dialogue"
label = "Quoted dialogue"
extensions = ["scn"]
detect_regex = []
min_units = 1
overwrite = false
skip_lines = ['^\s*[;#]']
notes = "Only literal dialogue lines, no escape syntax."

[[rules]]
kind = "line_regex"
pattern = '^(?P<role>[^：]+)：「(?P<text>[^」]+)」$'
```

`pattern` 为必填字符串，命名捕获 `text` 必须存在，`role` 可选。只替换 text 跨度，前后语法保留。没有专用 escape codec；若源语法允许转义引号、内嵌命令或翻译产生终结符，这种规则可能不安全。要描述合适的转义策略或改用专用适配器，不能说正则能解析任意编程语言。

### `json_keys`

```toml
[[rules]]
kind = "json_keys"
keys = ["message", "caption"]
```

`keys` 是字符串数组，按对象键递归选择字符串值或字符串数组。不要添加宽泛的 `id`、`path` 或逻辑名称键以提高提取量。

### `json_paths`

```toml
[[rules]]
kind = "json_paths"
paths = ["events/*/text", "**/choices/*"]
```

`paths` 是斜线分隔的路径 glob 数组。`*` 匹配一层，`**` 匹配任意层，不是 JSONPath 表达式。多类规则可组合，最后选择的是符合源语言过滤的单元。

## 模型推断

```bash
attx profile infer --input './script.scn' --output './inferred.toml' --src ja --name script-local
attx profile test --profile './inferred.toml' --input './script.scn' --roundtrip
```

显式 infer 使用当前模型，有费用，且不覆盖已有 output。它最多执行三次提案和验证，要求行规则两端锚定，拒绝零单元与明显机器字面值，强制 `overwrite=false`。程序只编译声明式规则，绝不执行模型返回的代码。

验证包含提取试验和将原文当作译文的 decoded-text 无损 no-op 往返。这可发现结构变化，不证明源编码字节完全一样，不证明模型理解语法，更不证明将来任意译文都能安全嵌入。Profile 输出为 UTF-8，行模式保留源换行。含复杂转义的脚本必须使用真正懂其语法的适配器。

`run` 在没有专用/保存 Profile/auto 匹配且未禁止推断、未跳过翻译时，会将通过验证的规则放在 `<workspace>/profile.toml`，后续运行复用。`--no-infer` 禁止自动付费推断。已有 workspace Profile 的字节内容不能偷偷更换；规则改变需要新工作区。

二进制、不可读、有损解码、模糊或不能可靠定位的语法会停止，并提示使用外部提取器和 JSONL。推断失败不是授权代理生成代码直接改输入。

## 保存规则与作用域

`profile list` 返回实际目录及规则。优先搜索 `$ATTX_HOME/profiles/`，再平台配置目录 `attx/profiles/`。保存后，其他输入可能匹配它，因此 `extensions`、`detect_regex` 和 `min_units` 应足够具体。

手工选择的 `overwrite=true` Profile 可以正常原地写回，无需再取得强制许可；但修改规则从副本扩大到原地覆盖是新的行为，应由用户明确选择。自动推断始终只输出副本。

继续：[格式边界](formats.md) · [工作区身份](workspace.md) · [添加适配器](development.md)。
