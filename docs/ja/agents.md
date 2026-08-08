# エージェント

Skill（`skills/attx/SKILL.md`）が実行プロトコルです。設定欠落時は Q&A で `setting.toml` を書き、キーはチャットに出しません。

```bash
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
```

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
Help me set up attx, then translate ./novel.epub from Japanese to Simplified Chinese.
```
