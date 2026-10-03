# Agent 自动化

attx 通过本地 CLI 的 JSON 提供自动化接口，不要求 MCP 服务器。仓库的 [`skills/attx/SKILL.md`](https://github.com/kaecho/attx/blob/main/skills/attx/SKILL.md) 是执行协议；人类也可以直接用[快速开始](quickstart.md)。

第一次使用、不想自己读配置文档或编辑 TOML 时，打开[Agent 翻译向导](agent-translation.md)，复制完整提示词。Agent 会自己读文档、安装或找到程序，逐步解释并询问 API、Key、模型和参数，再生成配置与执行任务。下面是已有配置后的执行协议说明。

## 安装 Skill

Claude Code 个人目录：

```bash
mkdir -p ~/.claude/skills
cp -a skills/attx ~/.claude/skills/
```

项目目录改为 `.claude/skills/`。PowerShell：

```powershell
New-Item -ItemType Directory -Force "$HOME\.claude\skills" | Out-Null
Copy-Item '.\skills\attx' "$HOME\.claude\skills\" -Recurse
```

已有安装不要未经选择就覆盖。其他支持 Skill 的 agent 按其目录约定安装；不支持时保留仓库并要求它读取真实 `skills/attx/SKILL.md`。安装了文本协议不等于已安装二进制，仍需可运行的 attx。

## 直接给出任务

```text
按 <attx目录>/skills/attx/SKILL.md 用 attx 翻译 <输入>，日文到简体中文。
配置为 <配置>。正常翻译和写回已授权，请自动完成并报告输出及未解决问题。
不要通过聊天索取密钥，不额外开启付费术语构建，不删除工作区。
```

输入、语言、模型与配置都已明确且有效时，默认直接执行完整任务。不把“先试译，等待许可，再全量”设为每个任务必经流程，不因输入规模大再强制请求一次授权。只有用户要求样本、预算或预览时才加相关限制。

```bash
attx --config './setting.toml' run --input './game' --src ja --dst zh
```

用户命名输入的翻译请求包括正常写回，含 RMMZ 原地写回和用户已选的 overwrite=true Profile。dry-run 可用于查看计划，不是强制许可闸门。授权不包括删除/重置工作区、改无关文件、替换其他作品的规则、关闭备份或把未完成任务偷偷变为部分输出。

## 配置缺失时

Agent 先查真实 CLI、已有配置和 doctor 结果，再按[问答配置协议](https://github.com/kaecho/attx/blob/main/skills/attx/references/agent-setup.md)补充缺失项。每一步解释是什么、能做什么、推荐值及影响。API Key 通过宿主安全输入或用户可操作的隐藏终端提供，由 Agent 自动写入本机配置，不要求手工编辑 TOML，不通过普通聊天收集或回显。

`doctor --json --ping` 的 ping 有小额模型请求。检查 llm.configured 和 ping 实际结果，顶层 ok 不是连通性证据。401/403、错误模型、错误端点等致命条件立即停止，不轮换 Key 或循环试连。

## 费用与范围

- 普通全文翻译与配置范围内的有限修复属于已授权任务。
- glossary 构建默认关闭，不额外启用 --glossary；已有配置明确启用或用户主动要求时可执行。
- LLM 经验复核默认关闭，不自行添加 --llm 或改 llm_review。
- run 无法识别格式时可自动付费推断 Profile；用户不接受这项费用时用 --no-infer。已知格式和 auto 的本地识别不产生推断请求。
- --allow-partial 需要明确接受部分输出，不能作为自动绕过 blocked 的办法。

不知道模型价格或吞吐时，不编造费用和时长。dry-run 只是规模计划，不是精确报价。

## 结果处理

按 stdout 最终 JSON 和进程码判断，stderr batch 进度不是完成证据。run 已实现共享翻译/审校/修复流程，不再在 agent 外层复制无限循环。

遇到 needs_attention 时报告 pending、passthrough、unresolved 和样例；到上限或无进展后停止重复请求，转 JSONL 校对。对 unknown format 应先 analyze，保守 auto 或声明式 Profile，再考虑外部提取器；绝不运行模型生成的代码。

review 样例最多 40 条，不是最多只有 40 个问题。auto 覆盖中 copied_files 是原样复制，不能算已译。RMMZ overflow_lines 即使不拦截写回也要报告。

## 经验审批和反馈

```bash
attx learn note --workspace './.attx' --name voice --text '保留角色之间的称呼差异。'
attx learn pending
```

作品要求放 workspace；全局 format 会影响未来项目，不能自动扩大作用域。专名走 glossary，变量走 preserve。学习不会自动改写已完成译文。

skip 提案可能删除提取内容，必须提交证据让用户按索引批准。代理不得自行执行 `learn review --approve-all`。模型复核不代替用户批准。

人工校对只改 JSONL 译文字段，经 import-jsonl 校验再写回。不直接编辑输入、输出、attx.db 或工具源码去制造通过结果。不要删掉真实日文以消除残留。

## 最终摘要

说明输入和工作区、真实输出路径、实际是否写回、成功与未解决数量、覆盖遗漏/布局问题及具体续处理命令。错误后已经提交的缓存可以保留，不能因此声称整任务完成。对样本、dry-run 或 no-writeback 明确标注范围。

按需读取[代理参考目录](https://github.com/kaecho/attx/tree/main/skills/attx/references)中的 CLI 契约、agent usage、未知格式、failure recovery、JSONL 和 feedback iteration，不把整个手册塞进模型翻译 prompt。

继续：[CLI](cli.md) · [质量](quality.md) · [工作区](workspace.md)。
