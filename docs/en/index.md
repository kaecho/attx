# attx

**Agent Translation Toolkit eXtensible** — extract → translate (any OpenAI-compatible LLM) → writeback.

One Rust binary. SQLite workspace. Resume for free.

## Why

- **Format adapters**, not one-off scripts: games, ebooks, docs, subtitles, l10n files
- **Agent-friendly**: JSON on stdout, ship-with Skill protocol
- **Safe defaults**: RPG Maker writes `*.attxbak`; documents write sibling `*.<dst>.*` copies

## 30-second path

```bash
cp setting.example.toml setting.toml   # API endpoint + key + model
attx doctor --ping
attx run --input novel.epub --src ja --dst zh
```

RPG Maker:

```bash
attx run --input /path/to/game --src ja --dst zh --no-writeback
attx writeback --workspace /path/to/game/.attx
```

## What's new in 0.7

| Area | Change |
|------|--------|
| MZ namebox | `code 101` `parameters[4]` → domain `namebox`, writeback supported |
| Message lines | Width-aware reflow into original `401` slots (CJK-safe, control codes intact) |
| plugins.js | Nested JSON-string params + param names with `/` write back correctly |

Continue: [Install](install.md) · [Usage](usage.md) · [RPG Maker](rmmz.md)
