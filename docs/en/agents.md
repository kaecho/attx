# Agents

attx is a local CLI with JSON on stdout — that is already the tool surface for coding agents. A **Skill** (markdown protocol) is enough; no MCP required.

## Install skill

```bash
# Claude Code (personal)
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
```

Other agents: keep the checkout and instruct:

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
```

## Suggested prompt

```text
Use attx at <attx-dir>, following skills/attx/SKILL.md, to translate
<input> from Japanese into Simplified Chinese.

1. Only use the attx CLI; do not hand-edit inputs or attx.db.
2. If LLM is unconfigured, run the Q&A wizard; never print the API key.
3. doctor --ping → detect → init → extract → status → translate --limit 20 → full.
4. Ask before in-place writeback (RPG Maker).
5. Report counts after each stage.
```
