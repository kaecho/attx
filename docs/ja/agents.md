# エージェント

attx はローカル CLI で stdout が JSON です。コーディングエージェントには **Skill**（Markdown プロトコル）で十分で、MCP は必須ではありません。

```bash
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
```

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
```
