# Agents

attx is a local CLI with JSON on stdout — already the tool surface for coding agents. The **Skill** is the execution protocol: staged pipeline, hard stops, and a Q&A configuration wizard. MCP is optional and usually unnecessary.

## Install the Skill

```bash
# Claude Code (personal, all sessions)
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
# project-scoped
mkdir -p .claude/skills && cp -a skills/attx .claude/skills/
```

Other agents (Cursor / Codex / OpenCode / …): keep the checkout and require:

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
```

Files:

```text
skills/attx/SKILL.md                                    # stages, hard stops, wizard
skills/attx/references/cli-command-contract.md          # exact CLI + JSON contract
skills/attx/references/agent-usage.md                   # session structure, pitfalls
skills/attx/references/custom-format-discovery.md       # unknown-format flow
skills/attx/references/failure-recovery.md              # symptom → action tables
skills/attx/references/jsonl-workflow.md                # JSONL interchange
skills/attx/references/feedback-iteration.md            # post-playtest feedback loop
```

## Q&A configuration wizard

Triggered when config is missing or `doctor --ping` fails. The agent asks **one item at a time**:

1. API endpoint (OpenAI / DeepSeek / custom `base_url` ending in `/v1`)
2. API key → write only to `setting.toml`, never print again, never into chat/logs/git
3. Model name
4. Source / target languages
5. Optional: concurrency, glossary on/off (glossary is off by default — it costs extra LLM calls)

Then: `attx doctor --ping` → stage 0 of the pipeline.

## The staged pipeline

`detect` → `init` → `extract` → `status` → trial `translate --limit 20` → full `translate` → `writeback` (dry-run first for in-place formats). After each stage, report counts and the next step.

Hard stops the Skill enforces: `doctor --ping` 401 (no key-retry spam), no adapter found (switch to the profile toolchain), systematic trial failures (stop, don't burn money), and **writeback that overwrites a game directory requires explicit user permission**.

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

## Golden rules for agents

- **stdout JSON is the result.** stderr `batch i/n` progress lines are not.
- **Never hand-edit** inputs, `attx.db`, or tool source — operate through the CLI.
- **Never echo the API key** — it lives only in `setting.toml`.
- `learn review --approve-all` is forbidden for agents: `skip` entries delete text, so report pending entries with their evidence and let the user decide.
- `status.passthrough > 0` at wrap-up must be reported; `--retry-passthrough` re-queues those units.
- Unknown format → `analyze` → `profile new`/`test` → `init --profile` (never give up at `detect` failure).
