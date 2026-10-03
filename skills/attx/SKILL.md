---
name: attx
description: >
  通用 AI 翻译执行协议。用户要求翻译游戏（RPG Maker MV/MZ、MTool、Ren'Py、
  VNText）、电子书、网页、文档、字幕、表格、本地化文件或其他文本格式时使用。
  读取已有配置，通过 attx CLI 自动探测、提取、翻译、有限修复并写回，报告未解决问题。
  未知文本先用 auto；必要时推断并验证声明式 Profile，保存后复用。
---

# attx 翻译执行协议

仓库：https://github.com/kaecho/attx
人类文档：`README.md`（英文）、`README.zh-CN.md`（中文）。命令细节见 `references/`。

## 授权与边界

用户要求翻译某个输入，就授权该输入的正常翻译和写回，包括 RMMZ 原地写回、已选 `overwrite=true` Profile 及断点续译。不要再问一次写回许可，也不要把先试译、确认后全量设为默认流程。用户明确要求只预览、只提取、先试译或指定预算时，按其限制执行。

- 通过 attx CLI 操作业务数据。不得直接修改输入、`attx.db`、`experience.toml` 或工具源码来绕过检查。
- 正常写回保留备份和源文校验；备份失败时停止。`--dry-run` 是可选预览，不能说成已经生成输出。
- 翻译请求不授权删除或重置工作区、改动无关文件、覆盖其他作品的 Profile、批量重译无关已通过条目。此类操作需要明确许可。
- 向用户询问 API Key 的安全提供方式，通过宿主安全输入或隐藏终端直接写入本机 `setting.toml`。由 Agent 生成配置，不要求用户手工编辑 TOML；不在普通聊天中索取或回显密钥，不输出整份私密配置或带凭据日志。
- 使用已有配置和用户已给出的语言、模型、路径、预算。只问确实缺失且本地无法确定的信息。
- 不擅自开启付费术语提取或 LLM 经验复核。已有配置明确启用时可沿用；显式术语构建请求也是授权。
- 提取学习可能提出会删除文本的规则。报告 `learned.pending`，由用户审阅；代理不得自行 `learn review --approve-all`。

## 开始任务

1. 找到真实 CLI 入口：PATH、发行包或已有 `target/release/attx`。用户要求自动配置而未安装时，读取安装文档并由 Agent 安装适合本机的发行版到用户范围；系统级变更或覆盖已有安装另行说明，不顺便改源码。
2. 确定输入、语向和配置路径。全局选项为 `--config <路径>`、`--client <名称>`。配置默认查找 `$ATTX_HOME/setting.toml`，否则 `./setting.toml`。
3. 用 `attx doctor --json` 读取配置状态；需要检查连接时加 `--ping`。检查 `llm.configured` 和 `ping`，不要只看顶层 `status` 或进程码。
4. 配置缺失/失效或用户要求重新设置时，先读安装、使用与配置说明，再按 `references/agent-setup.md` 逐步询问 API 地址、Key、模型、输入/语向、范围、预算和参数。每一步解释用途、影响和推荐值，由 Agent 写配置并校验；已有有效值不重复问。401/403 等致命错误停止请求，只修正必要项，不重复刷 Key。

工作区默认：目录输入 `<content_root>/.attx`，文件输入 `<父目录>/.attx-<文件名去扩展名>`。已有工作区绑定输入、引擎和语向，不能借用另一作品的数据库。

## 默认执行：共享流水线

```bash
attx --config <配置> run --input <输入> --src ja --dst zh
```

`run` 负责探测、初始化、增量提取、按配置构建术语表、翻译、机械审校、有限修复和写回。使用其结构化结果推进任务，不要再在代理层复制一套无限的 translate/review 循环。

- 尊重显式 `--workspace`、`--engine`、`--profile` 和语言参数。
- `--no-writeback` 用于用户要求只翻译不写文件；`--no-translate` 用于提取和检查。
- `--limit N` 用于用户要求的样本或预算限制，不意味着全量已完成。
- `--no-glossary` 可单次关闭配置中的付费术语构建；不要自动添加 `--glossary`。
- 默认拒绝未完成任务的部分写回。只有用户明确接受部分输出时使用 `--allow-partial`，并逐项报告剩余问题。
- 未知格式的模型 Profile 推断可用 `--no-infer` 关闭。这不关闭 `auto` 的本地文本探测。

报告提取规模和费用风险，但用户已授权全量时继续执行，不再因条目数较大强制问一次。没有模型价格和吞吐证据时，不编造费用或时长。

## 格式选择

`attx formats` 列出当前二进制支持的格式和已保存 Profile，不在本协议维护版本相关的完整清单。

- 专用适配器覆盖游戏、EPUB、HTML、DOCX/XLSX、TXT/Markdown、字幕、CSV/TSV 和本地化 JSON 等格式。
- 优先专用适配器和已保存的 `custom:<名称>` Profile，再使用 `auto` 内容探测。不要仅凭扩展名强制错误引擎；`detect` 本身没有 `--engine`，指定引擎要用 `init` 或 `run`。
- `auto` 保守处理可识别的 JSON、XML、标量配置和正文文本。目录可能混有不可处理的文件；报告覆盖情况和原样复制的文件，不把复制当成已翻译。
- `run` 无法匹配时可有限推断声明式 Profile；不得执行模型生成的代码。推断 Profile 必须 `overwrite=false`，通过试提取和源文无损往返验证后才能使用。
- 二进制、加密、无法可靠定位正文的结构应安全停止，说明需要的外部提取器或 JSONL 数据。不要为提高覆盖率把脚本、路径、变量和资源标识当作正文。

操作细节见 `references/custom-format-discovery.md` 和 `references/jsonl-workflow.md`。

## 断点、修复与审校

中断后复用同一输入和语向的工作区，重新 `run` 或 `translate`，保留已提交的源文匹配译文。不要删库解决普通失败。

```bash
attx status --workspace <工作区>
attx review --workspace <工作区>
attx repair --workspace <工作区>
```

修复有配置上限，针对 pending、passthrough、残留源文、控制符和姓名框问题；术语子串告警只作审阅辅助。不要把有限修复扩成无穷重试。停止进展或达到上限后，导出具体条目人工修正，或诚实报告未解决项。

审校读取 `residual_source`、`kana_edge`、`kana_mixed`、`kana_untranslated`、`identical`、`control_loss`、`namebox_mismatch` 和 `glossary.violations`。`pending=0` 不代表没有残留，`passthrough` 也不是成功译文。

中文目标的机械假名尾屑清理，以及 RMMZ 固定对白槽位重排，会同步到缓存后再用于写回和审校。不要直接编辑输出以制造缓存与游戏不一致，也不要删掉真实日文句子掩盖漏翻。写回报告中的规范化、重排和溢出信号应如实报告。

人工审校走 `export-jsonl`、修改译文字段、`import-jsonl`。保留原 `id` 和 `text`；导入校验当前源文身份，不接受未知 ID、过期源文或损坏控制符。反馈流程见 `references/feedback-iteration.md`。

## 结果判定

最终 stdout JSON 和进程结果是依据，stderr 进度不是完成证据。

- `ok`：本次请求的操作完成。仍需说明是否主动限制了范围、跳过写回或存在术语建议，不能承诺译文完美。
- `needs_attention`：还有 pending、passthrough、残留或其他待处理问题。报告数量、样例和能继续的具体命令，不宣称全量完成。
- `blocked` 或非零错误：当前条件不允许继续，例如致命配置错误、身份不符、备份失败或默认完整性检查拒绝写回。说明输出是否产生和已经保留的缓存进度。

给用户的摘要包括：输入/工作区、实际输出路径、已译与未解决数量、是否真实写回、下一步。可无人值守的任务不要在每个阶段停下来等确认。

## 经验与反馈

只记录有证据、符合用户要求的具体习惯：

```bash
attx learn note --workspace <工作区> --name voice --text "<一条具体翻译要求>"
```

作品内用 `--workspace`。全局 `--format` 会影响未来作品，不能自动扩大作用域。专有名词走 glossary，不用泛泛的 prompt note。学习不会自动重译已完成条目。

## 按需参考

| 工作 | 参考 |
|------|------|
| 命令、选项与 JSON 字段 | `references/cli-command-contract.md` |
| 安装 Skill、启动与无人值守执行 | `references/agent-usage.md` |
| 复制提示词、解释式问答、自动生成配置 | `references/agent-setup.md`、`docs/zh/agent-translation.md` |
| auto、Profile 推断与复用 | `references/custom-format-discovery.md` |
| 致命错误、断点和有限重试 | `references/failure-recovery.md` |
| 外部引擎和 JSONL 校对 | `references/jsonl-workflow.md` |
| 试玩、漏翻和显示反馈 | `references/feedback-iteration.md` |

仅阅读当前工作需要的章节，不把整份协议塞进翻译 prompt。

## 用户任务示例

```text
按 <attx目录>/skills/attx/SKILL.md 用 attx 翻译 <输入>，日文到简体中文。
配置为 <配置>。正常翻译和写回已授权，请自动完成并报告输出及未解决问题。
不要通过聊天索取密钥，不额外开启付费术语构建，不删除工作区。
```
