# Agent

attx 是本地 CLI，stdout 输出 JSON，可直接给编程 Agent 当工具。自带 **Skill**（Markdown 协议）即可，不必上 MCP。

## 安装 Skill

```bash
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
```

其他 Agent：保留仓库路径并要求：

```text
严格遵循 <attx-dir>/skills/attx/SKILL.md
```

## 推荐提示词

```text
使用 <attx-dir> 的 attx，遵循 skills/attx/SKILL.md，
将 <input> 从日文翻译为简体中文。

1. 只通过 attx CLI 操作，不手改输入或 attx.db
2. 未配置 LLM 时走问答向导；不要打印 API key
3. doctor --ping → detect → init → extract → status → translate --limit 20 → 全量
4. RPG Maker 原地写回前先确认
5. 每阶段汇报数量
```
