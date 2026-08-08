# RPG Maker MV/MZ

attx is a **general** translation framework — this page is only the RMMZ-specific notes. For the main flow see [Usage](usage.md) and [Agents](agents.md).

The `rmmz` adapter takes a **game directory**. It detects content roots `./`, `www/`, or `game/` (needs `data/` plus `System.json` or `js/`).

## Pipeline

```bash
attx detect  --input /path/to/game
attx init    --input /path/to/game --src ja --dst zh      # workspace: /path/to/game/.attx
attx extract --workspace /path/to/game/.attx
attx translate --workspace /path/to/game/.attx
attx writeback --workspace /path/to/game/.attx --dry-run  # preview paths[]
attx writeback --workspace /path/to/game/.attx
```

**Writeback is in-place**: `data/*.json` and `js/plugins.js` are rewritten in the game directory, each overwritten file backed up once as `*.attxbak`. Always dry-run and confirm first — an agent must ask the user before a real writeback.

## What is extracted

| domain | Source |
|--------|--------|
| `dialogue` | Show Text event command (`401`) |
| `namebox` | MZ speaker plate — event command `101` `parameters[4]` |
| `choices` | Show Choices event command (`102`) |
| `scroll` | Scrolling Text event command (`405`) |
| `system` | `System.json` — terms, messages, menus |
| `base` | Database names / profiles / descriptions (`Actors.json`, `Skills.json`, …) |
| `plugins` | `js/plugins.js` — plugin `@param` values only, **never** plugin source files |

`\N[n]` namebox references are not extracted. Names in `data/*.json` referenced elsewhere are skipped by learned rules (see the `plugins` domain in the built-in experience: `attx learn defaults --format rmmz`).

## Plugin params

- Only `js/plugins.js` is touched — `.js` plugin sources are never modified.
- Parameters whose values are nested JSON strings are decoded on extract and re-encoded on writeback, so nested structures round-trip.
- Parameter names containing `/` round-trip as well.
- Message lines are reflowed by display width back into the original number of `401` slots.

## After writeback

Experience learned from the run applies to the next project automatically (see the README's *Self-improving experience layer*). `*.attxbak` files let you restore the pre-translation state; they are gitignored.
