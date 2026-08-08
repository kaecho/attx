# attx

**English** | [中文](README.zh-CN.md) | [Docs](https://emptysuns.github.io/attx/)

**Agent Translation Toolkit eXtensible** — one Rust binary that extracts text, translates it with any OpenAI-compatible LLM, and writes the result back.

```text
extract → translate → writeback
```

Progress lives in a SQLite workspace, so you can stop and resume for free.

## Install

- **Binary:** [Releases](https://github.com/emptysuns/attx/releases) (`v*`)
- **From source:**

```bash
cargo install --path .
# or
cargo build --release && ./target/release/attx --help
```

## Quick start

```bash
cp setting.example.toml setting.toml   # set base_url / api_key / model
attx doctor --ping

# one-shot (ebook / doc / subtitle → sibling *.<dst>.* file)
attx run --input novel.epub --src ja --dst zh

# RPG Maker MV/MZ (in-place + *.attxbak)
attx run --input /path/to/game --src ja --dst zh --no-writeback
attx writeback --workspace /path/to/game/.attx --dry-run
attx writeback --workspace /path/to/game/.attx
```

Step-by-step (large projects):

```bash
attx detect  --input <path>
attx init    --input <path> --src ja --dst zh
attx extract --workspace .attx-<name>
attx status  --workspace .attx-<name>
attx translate --workspace .attx-<name> --limit 20   # trial
attx translate --workspace .attx-<name>              # full / resume
attx writeback --workspace .attx-<name>
```

## What it supports

| id | Input | Notes |
|----|-------|--------|
| `rmmz` | game folder | MV/MZ `data/*` + `js/plugins.js` (plugin **source** never touched) |
| `epub` / `html` / `docx` / `xlsx` | file | layout preserved |
| `txt` / `md` | file | line / block units |
| `srt` / `vtt` / `ass` / `lrc` | file | timings kept |
| `po` / `renpy` / `csv` | file | gettext / Ren'Py / tables |
| `mtool` / `paratranz` / `vnt` / `i18next` | `.json` | content-sniffed |
| `jsonl` / `custom:<name>` | file/dir | escape hatch + TOML profiles |

`attx formats` lists everything (including saved profiles).

## RPG Maker highlights (0.7+)

- **Namebox** (`code 101` / `parameters[4]`) extracted as domain `namebox` and written back
- **Message reflow** — long CJK lines reflow into the original number of `401` slots by display width
- **Plugins writeback** — nested JSON-string params (and param names containing `/`) round-trip correctly

## Agent skill

Ship-with protocol for coding agents:

```bash
cp -a skills/attx ~/.claude/skills/    # Claude Code
# others: "Strictly follow <attx>/skills/attx/SKILL.md"
```

## Docs

Full guide (EN / 中文 / 日本語): **https://emptysuns.github.io/attx/**

## License

MIT
