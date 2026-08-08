# attx

**Agent Translation Toolkit eXtensible** — extract → translate (any OpenAI-compatible LLM) → writeback.

One Rust binary. Format-agnostic. SQLite workspace so you can stop and resume for free.

## Start with an agent (fastest)

1. Install the binary ([Releases](https://github.com/emptysuns/attx/releases) or `cargo build --release`)
2. Install the Skill: `cp -a skills/attx ~/.claude/skills/`
3. Tell the agent:

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
Help me set up attx if needed, then translate <input> from Japanese to Simplified Chinese.
```

The Skill runs a **Q&A wizard** (endpoint, API key, model, languages) when `setting.toml` is missing, writes the key only to disk, then runs `doctor --ping` → detect → extract → trial translate → full run.

## Or do it yourself

```bash
cp setting.example.toml setting.toml   # fill base_url / api_key / model
attx doctor --ping
attx run --input novel.epub --src ja --dst zh
```

## What it covers

Ebooks, documents, subtitles, localization JSON/PO, Ren'Py, RPG Maker, custom TOML profiles for unknown formats — see [Formats](formats.md).

Continue: [Install](install.md) · [Agents](agents.md) · [Usage](usage.md)
