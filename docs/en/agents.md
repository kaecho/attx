# Agents

attx is a local CLI with JSON on stdout — already the tool surface for coding agents. The **Skill** is the execution protocol (stages, hard stops, Q&A config). MCP is optional and usually unnecessary.

## Install the Skill

```bash
# Claude Code (personal)
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
# project-scoped
mkdir -p .claude/skills && cp -a skills/attx .claude/skills/
```

Other agents: keep the checkout and require:

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
```

Files:

```text
skills/attx/SKILL.md
skills/attx/references/   # CLI contract, discovery, recovery, JSONL, feedback
```

## Q&A configuration wizard

Triggered when config is missing or `doctor --ping` fails. The agent asks one item at a time:

1. API endpoint (OpenAI / DeepSeek / custom `base_url` ending in `/v1`)
2. API key → write only to `setting.toml`, never print again
3. Model name
4. Source / target languages
5. Optional: workers, glossary on/off

Then: `attx doctor --ping` → stage 0 of the pipeline.

## Copy-paste prompt

```text
Use the attx toolkit at <attx-dir>, following skills/attx/SKILL.md.

Help me set up attx if needed (Q&A: endpoint, key, model, languages),
then translate <input path> from Japanese into Simplified Chinese.

Rules:
1. Only the attx CLI; never hand-edit inputs, attx.db, or tool source.
2. Never print my API key.
3. doctor --ping → detect → init → extract → status → translate --limit 20 → full.
4. Prefer translated copies; ask before any in-place overwrite.
5. Report counts after each stage.
```

Short form:

```text
Help me set up attx, then translate ./novel.epub from Japanese to Simplified Chinese.
```
