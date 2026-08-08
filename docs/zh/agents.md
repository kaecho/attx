# Agent

attx 是本地 CLI，stdout 为 JSON，可直接给编程 Agent 当工具。**Skill** 是执行协议（阶段、硬停止、问答配置）。一般不需要 MCP。

## 安装 Skill

```bash
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
```

其他 Agent：

```text
严格遵循 <attx目录>/skills/attx/SKILL.md
```

## 问答式配置向导

配置缺失或 `doctor --ping` 失败时触发。Agent 逐项问：

1. API 端点（OpenAI / DeepSeek / 自定义 `base_url`）
2. API Key → 只写入 `setting.toml`，禁止回显
3. 模型名
4. 源/目标语言
5. 可选：并发、术语表

然后 `attx doctor --ping`，进入流水线。

## 可复制提示词

```text
请使用 attx（目录：<attx目录>），遵循 skills/attx/SKILL.md。
若未配置，先问答写入 setting.toml，再把 <输入> 从日文译为简体中文。

1. 只走 attx CLI；不手改输入或 attx.db
2. 不要打印 API Key
3. doctor --ping → detect → init → extract → status → limit 20 → 全量
4. 文件类出副本；原地覆盖前先问我
5. 每阶段汇报数量
```
