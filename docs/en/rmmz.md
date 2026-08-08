# RPG Maker MV/MZ

Engine id: `rmmz`. Input is a **game directory** (`data/` + usually `js/`).

## Pipeline

```bash
attx detect  --input /path/to/game
attx init    --input /path/to/game --src ja --dst zh
attx extract --workspace /path/to/game/.attx
attx translate --workspace /path/to/game/.attx
attx writeback --workspace /path/to/game/.attx --dry-run
attx writeback --workspace /path/to/game/.attx
```

Writeback is **in-place** and creates `*.attxbak` next to touched files.

## Domains

| domain | Source |
|--------|--------|
| `dialogue` | Show Text body (`401` lines after `101`) |
| `namebox` | MZ speaker plate — `101` `parameters[4]` (0.7+) |
| `choices` | Show Choices (`102`) |
| `scroll` | Scrolling text (`405`) |
| `system` | `System.json` terms / title |
| `base` | Actors / Items / Skills / … names & descriptions |
| `plugins` | `js/plugins.js` parameters only (never `js/plugins/*.js`) |

`\N[n]` namebox values are **not** extracted (runtime actor names).

## Message line reflow (0.7+)

Translations often return one long line while the event still has 3× `401` slots. Writeback reflows by **display width** (ASCII≈1, CJK≈2, control codes≈0) into exactly `n` slots. Control codes such as `\C[27]` are never split.

Default max width: **44**. Equal-length, non-overflow lines are left unchanged.

## Plugin parameters

- Nested values stored as **JSON strings** (sometimes several layers) are decoded on extract and re-encoded on writeback.
- Param names may contain `/` (e.g. `行動-表示位置/味方`); locations keep the full name, payload carries `plugin_index` / `param` / `json_path`.
- Identity / path fields (`key`, `FilePath`, …) are skipped.

## Tips

1. Trial-translate 20 units, open the game, then full run.
2. Prefer `writeback --dry-run` before the first real write.
3. If a plugin breaks after writeback, restore `js/plugins.js.attxbak` and open an issue with the plugin name + location.
