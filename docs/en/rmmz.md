# RPG Maker MV/MZ

The `rmmz` adapter works on a game directory. It locates a content root at the input itself, `www/`, or `game/`, requiring `data/` and a recognized project shape such as `System.json` or `js/`. It is not support for every RPG Maker generation or every proprietary plugin grammar.

## Normal pipeline

```bash
attx detect --input "./My Game"
attx --config "./setting.toml" run --input "./My Game" --src ja --dst zh
```

Read detection's `content_root`: a packaged game's workspace can be `My Game/www/.attx`, not the directory you originally typed. For a fixed location:

```bash
attx init --input "./My Game" --src ja --dst zh --workspace "./My Game translation"
attx extract --workspace "./My Game translation"
attx --config "./setting.toml" translate --workspace "./My Game translation"
attx review --workspace "./My Game translation"
attx writeback --workspace "./My Game translation" --dry-run
attx --config "./setting.toml" writeback --workspace "./My Game translation"
```

RMMZ writes in place to live data JSON and `js/plugins.js`. Each existing destination has a first-write backup, for example `data/Map001.json.attxbak`. Backup failure stops replacement. The normal translation request authorizes this format's writeback; there is no mandatory second permission prompt. Preview-only requests remain preview-only.

## Extraction domains

| Domain | Source |
|---|---|
| `dialogue` | Show Text command `401` groups in maps, common events, and troops |
| `namebox` | MZ Show Text command `101`, speaker field `parameters[4]` |
| `choices` | Show Choices command `102` |
| `scroll` | Scrolling Text command `405` |
| `system` | Supported visible terms/messages/menu strings in `System.json` |
| `base` | Supported names, profiles, descriptions, and messages in database records |
| `plugins` | Supported human-facing parameter values in `js/plugins.js` |

Database extraction distinguishes player-facing text from identifiers. It does not translate every string key or change numeric event logic. Name references such as `\N[n]` are protected engine syntax, not independent speaker text. Embedded [experience defaults](usage.md) provide conservative field handling; they do not make all plugin parameters safe.

## Original data and repeat writeback

Re-extraction prefers `data_origin/` when available, then the matching `.attxbak` source, then live original data. Source snapshots stop translated live files from being mistaken for a fresh Japanese input. Writeback targets live `data/`, not `data_origin/` or backup files.

Source identity and anchors are checked before writing. Added or changed source units require extraction again rather than applying stale cache. Keep a separate pristine game copy for important projects. `.attxbak` is the first existing destination, not a rolling history of every translation revision.

Live structural checks cover event topology/order/codes/indent/nontext parameters, name/page relationships, database IDs/text shape, and plugin identity/order/status/parameter nesting. Safe unrelated coordinates, gameplay values, and descriptions remain live. Changes to structural conditions or identity parameters require extraction refresh. Swapping structurally indistinguishable text-only slots is not always detectable through a text-excluding projection, so retain pristine source evidence.

Default writeback blocks pending, passthrough, and invalid units. With deliberately accepted `--allow-partial`, valid rows are written while unresolved rows retain or restore their original source values. A repeat partial write does not silently leave an old invalid live translation in place. The result still exits 2.

## Plugin parameters

Only `js/plugins.js` is an output target. The adapter can inspect plugin header parameter types to distinguish display text from machine values, but it never rewrites plugin source `.js` files. Nested JSON strings are decoded for extraction and encoded again for writeback; supported slash-containing parameter names keep their locations.

Asset paths, script snippets, code, IDs, switches, and machine-like values are not ordinary dialogue. If a plugin displays text that does not appear in extraction, inspect the actual field and evidence. A learned `extract` rule cannot create units the adapter omitted. Use an explicit adapter extension or JSONL extraction workflow when the plugin's serialization is outside supported parameter shapes.

Ordinary unsupported plugin text can be absent from extraction; a completed run is not proof that all plugin UI text was selected. Integrity failures, including unsafe plugin-source evidence, are fatal rather than silently skipped. Inspect `status.domains`, exported units, and the game itself.

## Fixed dialogue slots

RPG Maker event command lists have structural meaning. attx preserves the existing command count and reflows translated dialogue into the original number of `401` slots instead of adding commands and shifting event indices.

Reflow uses display width and punctuation-aware boundaries, with 44 halfwidth cells as the layout threshold. Engine controls are not split as ordinary visible characters. A protected first-slot speaker label remains in that slot. If only two slots are available for a speaker label and body, the remaining body is kept, including unavoidable overflow, rather than truncated.

`writeback.reflowed_units` records affected units. `overflow_lines` reports unavoidable layout overflow, including a crowded final slot. These are layout signals, not a promise that every custom font or message plugin fits. Overflow must not be hidden by deleting text.

The normalized/reflowed text is persisted back into the translation cache only after actual successful artifact writeback. Subsequent exports and reviews see the text that was written. A blocked or dry-run writeback does not mutate that cache.

## Playtest and repair

```bash
attx review --workspace "./My Game translation"
attx --config "./setting.toml" repair --workspace "./My Game translation"
attx export-jsonl --workspace "./My Game translation" --output "./game review.jsonl" --filter all
```

Inspect speaker plates, menu labels, choices, scrolling text, and long dialogue using the game's real font and plugins. Review reports check controls, source-script residue, and speaker/name relationships, but cannot exercise the game runtime or verify semantics.

For human corrections, edit only JSONL translation fields, import them, and run writeback again. Do not directly edit cached SQLite rows or manually delete Japanese strings to clear a warning. Add concrete workspace prompt notes or glossary mappings before future requests when they express a reusable requirement.

Related pages: [quality and repair](quality.md), [workspaces and backups](workspace.md), [agents](agents.md), and [troubleshooting](troubleshooting.md).