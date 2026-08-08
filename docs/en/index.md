# attx

**Agent Translation Toolkit eXtensible** — a pure-Rust, single-binary, format-agnostic AI translation framework for coding agents and humans.

```
extract (format adapter) → translate (LLM core) → writeback (format adapter)
```

Translate games (RPG Maker MV/MZ, Ren'Py, MTool), ebooks (EPUB), documents (DOCX/XLSX/TXT/MD), subtitles (SRT/VTT/ASS/LRC), and localization files (PO, i18next, Paratranz, VNTextPatch) with **any OpenAI-compatible LLM**. Progress is cached in a SQLite workspace, so interrupted runs resume for free.

## What makes it different

- **Agent-first.** attx is a local CLI that speaks JSON on stdout — the native tool surface for coding agents. The Skill (`skills/attx/`) is the execution protocol: staged pipeline, hard stops, and a Q&A configuration wizard. No MCP server needed.
- **19 built-in adapters** plus **custom format profiles** (`line_regex` / `json_keys` / `json_paths` TOML rules) for anything else.
- **Resumable by design.** Every unit is checkpointed in `attx.db`. Re-run `translate` to continue; only pending units are sent to the model.
- **Honest failure.** Units the model keeps failing become visible *passthrough* placeholders — the run finishes, and `--retry-passthrough` re-queues exactly those.
- **Self-improving.** Successful runs leave extraction experience behind, reviewed by you before anything is ever deleted.

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
attx run --input novel.epub --src ja --dst zh   # → novel.zh.epub
```

## What it covers

Ebooks, documents, subtitles, localization JSON/PO, Ren'Py, RPG Maker, custom TOML profiles for unknown formats — see [Formats](formats.md).

Continue: [Install](install.md) · [Agents](agents.md) · [Usage](usage.md)
