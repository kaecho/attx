# 配置参考

配置文件是 TOML。模板为仓库或发行包的 [`setting.example.toml`](https://github.com/kaecho/attx/blob/main/setting.example.toml)，解析与默认值在 [`src/config.rs`](https://github.com/kaecho/attx/blob/main/src/config.rs)。这里列出所有当前键，不把示例值当成供应商推荐值。

## 查找顺序与覆盖范围

1. 提供 `--config <path>` 时只选该路径，不再搜索其他位置。
2. 未指定时，若设置了 `ATTX_HOME` 且其 `setting.toml` 存在，选择该文件。
3. 否则选择当前工作目录的 `setting.toml`，不是二进制所在目录。

选定文件不存在时，程序允许加载空客户端和其他默认配置，使离线命令仍能运行。显式路径不存在也不会自动回退；翻译时会因没有可用客户端失败。文件存在但 TOML 错误则报告解析错误。

`--client <name>` 只覆盖本次客户端选择，不合并多份配置。命令中的语言写进工作区，不是配置键。没有通用的环境变量到任意配置字段的映射，`api_key` 也不是自动展开的环境变量引用。

`ATTX_HOME` 还影响全局 Profile 和经验目录：

- 搜索 `$ATTX_HOME/profiles/`，再搜索平台用户配置目录的 `attx/profiles/`；同名 Profile 先找到者优先。
- 搜索 `$ATTX_HOME/knowledge/`，再搜索平台用户配置目录的 `attx/knowledge/`。
- 保存全局数据时使用第一个可用目录；没有 `ATTX_HOME` 时通常是平台用户配置目录。Linux 通常为 `~/.config/attx`，Windows 通常为 `%APPDATA%\attx`，macOS 通常为 `~/Library/Application Support/attx`。
- 不会把工作区迁入 `ATTX_HOME`，也不会自动从平台配置目录读取 `setting.toml`。

Linux/macOS：`export ATTX_HOME="$HOME/.attx-home"`。PowerShell：`$env:ATTX_HOME = "$HOME\attx-home"`。已有工作区仍位于其原路径。

## `[llm]`

已有配置必须提供这个小节。缺少整个配置文件和缺少现有文件中的 `[llm]` 不是同一回事。

| 键 | 类型 | 默认 | 效果 |
|----|------|------|------|
| `default_client` | 字符串 | 必填 | 默认客户端的 `name` |
| `clients` | 客户端数组 | 必填 | 使用一个或多个 `[[llm.clients]]` 声明 |

`default_client` 应准确匹配一个客户端，名字建议唯一。程序选择第一个匹配项，不会自动轮换密钥或故障切换。

## `[[llm.clients]]`

| 键 | 类型 | 默认 | 效果 |
|----|------|------|------|
| `name` | 字符串 | 必填 | 本地选择名称，供 `--client` 使用 |
| `provider_type` | 字符串 | `"openai"` | 仅保存元数据，不改变请求协议 |
| `base_url` | 字符串 | 必填 | 拼接 `/chat/completions` 的端点，不要写入该后缀 |
| `api_key` | 字符串 | 必填 | Bearer 凭据；必须在本机填写有效值 |
| `model` | 字符串 | 必填 | 供应商实际模型 ID |
| `timeout` | 非负整数，秒（u64） | `600` | 单次 HTTP 超时，实际最小值为 30 秒 |
| `temperature` | 浮点数，可省略 | 翻译 `0.3`，JSON 询问 `0.0` | 显式值覆盖翻译和术语/学习 JSON 请求的调用默认值 |
| `reasoning_effort` | 字符串，可省略 | 不发送 | 直接发送给支持该字段的服务 |
| `max_tokens` | 非负整数（u32），可省略 | 不发送 | 输出 token 上限，不等于输入批次预算 |
| `stream` | 布尔 | `false` | 发送流式请求，拼接 SSE 的 `delta.content` |
| `extra` | TOML 表 | 空表 | 最后合并到请求体，可新增或覆盖字段，唯独忽略 `messages` |

所有客户端都使用 OpenAI 兼容 Chat Completions。写成 `provider_type="anthropic"` 不会转换为 Anthropic Messages API；同理它不自动支持任何原生 SDK。服务不支持可选字段时，应删掉该字段，而不是改供应商名称碰碰运气。

`extra` 优先于顶层请求字段，包括 `model`、`temperature`、token 参数和 `stream`。这样也可能覆盖出与预期不同的模型或响应模式，故优先用明确的普通键。`messages` 不允许被替换。不要在 `extra` 中放密钥或用户输入模板。

```toml
[[llm.clients]]
name = "reasoning"
base_url = "https://api.example.com/v1"
api_key = "YOUR_API_KEY"
model = "provider-reasoning-model"
timeout = 600
reasoning_effort = "medium"
# 新模型如要求 max_completion_tokens，省略 max_tokens
extra = { max_completion_tokens = 8192 }
```

`stream` 不把完整翻译过程变成实时写回。仍需完整组装并验证输出；截断、缺失、重复编号或损坏保护标记不能作为成功。服务是否支持温度、推理字段和流式内容由其自身协议决定。

## `[translation]`

整个小节或其中任何键可省略，使用以下默认值。

| 键 | 类型 | 默认 | 效果 |
|----|------|------|------|
| `worker_count` | 非负整数（usize） | `8` | 同步 HTTP 工作线程数，实际至少1且不超过批次数；吞吐受限速和模型速度影响 |
| `rpm` | 非负整数（u32） | `60` | 共享请求开始间隔约为 `60/rpm` 秒；`0` 不限速 |
| `retry_count` | 非负整数（u32） | `3` | 一次翻译 pass 中每个单元最多额外请求次数 |
| `retry_delay` | 非负整数，秒（u64） | `2` | 重试前等待 |
| `batch_chars` | 非负整数（usize） | `2500` | 每批源字符预算；0使非空单元通常各自成批，不表示不翻译；不是 token 数或上下文窗口 |
| `max_context_items` | 非负整数（usize） | `6` | 每批翻译单元数上限，实际至少1；不是邻句条数 |
| `repair_rounds` | 非负整数（usize） | `2` | `translate`/`run` 初译后最多附加修复 pass；`0` 禁止自动附加轮 |
| `context_chars` | 非负整数（usize） | `1200` | 每个请求 prev/next 邻文总字符预算，含标签；`0` 不注入邻文 |

通常把 worker、batch、items 设为正数。批次遇到字符或条数上限就切分，单个超长单元不应被误解为自动截断文本。邻文按自然位置顺序选择，受文件和场景边界限制，可能含已提交译文；不能保证模型看到整部作品。

初次、重试和缩小失败批次的请求共享限速器。网络错误、408、429 和 5xx 有限重试；永久 4xx（如 400/401/403）立即使任务失败并抑制排队请求，已在途 HTTP 可能完成。格式或质量错误只重试失败单元，成功单元不重复请求。

每个单元每个 Translator pass 的上限是 `1 + retry_count` 次请求。`translate`/`run` 包含初译和最多 `repair_rounds` 个修复 pass。显式 `repair` 至少运行一轮，最多 `max(1, repair_rounds)` 轮；因此 `repair_rounds=0` 不会禁用用户主动执行的 `repair`。已无修复候选时提前结束。这是请求边界，不是精确的价格估算。

## `[glossary]`

| 键 | 类型 | 默认 | 效果 |
|----|------|------|------|
| `enabled` | 布尔 | `false` | `run` 翻译前构建术语表，有额外模型费用 |
| `min_occurrences` | 非负整数（usize） | `10` | 实际至少1；包含候选源文的单元数量门槛，同单元多次命中只计一次，姓名框候选可豁免门槛 |
| `max_terms` | 非负整数（usize） | `200` | 实际至少1；姓名框候选优先，其余按出现单元数排序后截断 |
| `inject_limit` | 非负整数（usize） | `30` | 单个翻译批次最多注入的相关 active 术语；0不注入 |

`run --glossary` 单次开启，`--no-glossary` 单次关闭，二者不能同时传。显式 `glossary build` 不受 `enabled=false` 限制。关闭构建不会禁用已有术语表的注入。构建最多抽取 40 批，每批源字符约 3500，大型作品不保证全文覆盖。报告 `truncated` 专指因 `max_terms` 截掉的候选数量，不反映40批之外漏读的源文。术语匹配是子串启发式，违规建议不作为硬门禁。结构和人工编辑见[工作区页](workspace.md)。

## `[learn]`

| 键 | 类型 | 默认 | 效果 |
|----|------|------|------|
| `auto_summarize` | 布尔 | `true` | 成功实际写回后根据已有数据库证据总结经验，不发模型请求 |
| `llm_review` | 布尔 | `false` | 总结时额外请求模型复核提案，有额外费用 |

`writeback --no-learn` 跳过本轮总结。会删除提取内容的 skip 提案仍需用户按索引审阅，模型复核不等于批准。经验不会自动重译已有译文。

## 三套实用参数

以下仅替换对应小节，不要覆盖已有密钥配置。它们是起点，不保证某服务的吞吐或译文质量。

### 低费用样本

```toml
[translation]
worker_count = 1
rpm = 10
retry_count = 1
retry_delay = 2
batch_chars = 1500
max_context_items = 4
repair_rounds = 0
context_chars = 400

[glossary]
enabled = false
[learn]
auto_summarize = true
llm_review = false
```

搭配 `run --limit 20 --no-writeback --no-glossary --no-infer`。减少修复次数也意味着更多问题需要人工处理，不是免费提高质量。

### 保守接口负载

```toml
[translation]
worker_count = 2
rpm = 20
retry_count = 3
retry_delay = 5
batch_chars = 1800
max_context_items = 4
repair_rounds = 2
context_chars = 1000
```

适合服务并发额度较小或长输出不稳定时作为初始值。先修正鉴权和模型问题，降低并发不能解决永久 4xx。

### 大项目分段续译

```toml
[translation]
worker_count = 4
rpm = 40
retry_count = 3
retry_delay = 3
batch_chars = 2500
max_context_items = 6
repair_rounds = 2
context_chars = 1200
```

保留同一个工作区，分段执行 `translate --limit N` 再续译，必要时人工维护术语表。不要为了大项目把 `rpm=0` 或 worker 数提高到未经服务额度支持的值。

## 密钥和数据

`api_key` 不写进工作区数据库，不作为翻译 prompt 内容。配置含凭据，虽然仓库忽略 `setting.toml`，其他命名的配置、日志或打包目录仍可能泄露。不要提交它们，不要打印整份私密配置，备份时设置本机适当权限。模型服务会收到原文、相关上下文、术语和 prompt note，应按源文件敏感程度选择服务。

本地服务明确不需要鉴权时，保留 `api_key` 字段但设为空字符串，不要编造 dummy Key。程序拒绝含配置密钥字符串的模型内容，并对 HTTP 错误中的匹配密钥脱敏；不应依靠这一保护去公开原文或整份配置。

未知格式自动推断是 `run` 在无法匹配且允许翻译时的额外模型请求，不由上述 glossary/learn 开关控制。禁止这项费用使用 `--no-infer`；显式 `profile infer` 本身授权推断请求。见[Profile 页](profiles.md)。

下一步：[CLI](cli.md) · [故障排查](troubleshooting.md)。
