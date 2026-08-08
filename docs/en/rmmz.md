# RPG Maker MV/MZ

One of many adapters (`rmmz`). Input is a **game directory** (`data/` + usually `js/`).

attx is a **general** translation framework — this page is only the RMMZ-specific notes. For the main flow see [Usage](usage.md) and [Agents](agents.md).

## Pipeline

```bash
attx detect  --input /path/to/game
attx init    --input /path/to/game --src ja --dst zh
attx extract --workspace /path/to/game/.attx
attx translate --workspace /path/to/game/.attx
attx writeback --workspace /path/to/game/.attx --dry-run
attx writeback --workspace /path/to/game/.attx
```

Writeback is **in-place** with `*.attxbak`. Always dry-run and confirm first.

## Domains

| domain | Source |
|--------|--------|
| `dialogue` | Show Text body (`401`) |
| `namebox` | MZ speaker plate — `101` `parameters[4]` (0.7+) |
| `choices` / `scroll` | Show Choices / scrolling text |
| `system` / `base` | System.json / DB names |
| `plugins` | `js/plugins.js` parameters only |

`\N[n]` namebox values are not extracted. Nested plugin JSON strings and param names containing `/` round-trip on writeback (0.7+). Message lines reflow by display width into the original number of `401` slots.
