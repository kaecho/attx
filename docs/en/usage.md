# Workflows

Use `run` for the normal pipeline. Separate commands are useful when you want to inspect extraction, set terminology before requests, exchange human-reviewed translations, or stop before writing files. Commands below use quoted paths and an explicit workspace.

## Translate and resume

```bash
attx --config "./setting.toml" run --input "./book.epub" --src ja --dst zh
```

This detects a format, initializes or reopens its workspace, extracts source text, optionally builds a glossary, translates with bounded repair, and writes validated artifacts. A file normally gets workspace `./.attx-book`; a directory uses its detected content root's `.attx`. Output naming is [format-specific](formats.md).

Re-run the same command after interruption. Source-matching completed entries remain cached. Pending and mechanically flagged entries can need new requests; resumption is not a promise of zero cost. Do not delete the database to handle an ordinary provider failure.

For a stepwise workflow:

```bash
attx detect --input "./book.epub"
attx init --input "./book.epub" --src ja --dst zh --workspace "./book workspace"
attx extract --workspace "./book workspace"
attx status --workspace "./book workspace"
attx --config "./setting.toml" translate --workspace "./book workspace"
attx review --workspace "./book workspace"
attx writeback --workspace "./book workspace" --dry-run
attx --config "./setting.toml" writeback --workspace "./book workspace"
```

The preview is optional, not a permission gate. `writeback --dry-run` can be blocked if the workspace is incomplete, and never writes outputs or changed normalized translations. Actual writeback verifies current source identity and anchors, stages artifacts, checks backups, and then replaces files.

## Deliberate limits and partial output

```bash
attx --config "./setting.toml" run --input "./book.epub" --src ja --dst zh \
  --workspace "./book workspace" --limit 20 --no-writeback
```

`--limit` constrains selected units, not an exact token price. The report still reflects the entire workspace's pending or unresolved entries, so a trial can return 2. A sample is not mandatory when the user already authorized a full run.

`--no-translate` extracts and reports without translation, normal writeback, glossary construction, or automatic paid profile inference. `--no-writeback` saves translations but produces no artifact. `--no-infer` prevents paid unknown-format inference without disabling local Auto detection.

Only use partial output when you accept untranslated originals:

```bash
attx writeback --workspace "./book workspace" --allow-partial
```

This keeps valid translations and restores/retains source values for unresolved units. It reports `needs_attention` and exits 2. It does not convert an incomplete project into a complete result.

## A workspace glossary

Glossary entries establish names for characters, places, items, or technical terms. Add trusted terms manually without HTTP:

```bash
attx glossary add --workspace "./book workspace" --src "アレイ" --dst "艾蕾" --info "female character"
attx glossary add --workspace "./book workspace" --src "May" --dst "梅" --info "character name" --case-sensitive
attx glossary list --workspace "./book workspace"
attx glossary check --workspace "./book workspace"
```

Build terms with the model when you explicitly want the extra requests:

```bash
attx --config "./setting.toml" glossary build --workspace "./book workspace" --dry-run
attx --config "./setting.toml" glossary build --workspace "./book workspace" --min-occurrences 10
```

The dry-run is a plan, not a quotation or a glossary. Construction has at most 40 extraction batches and a term cap. Real occurrence counts mean units containing the term, not repeated matches within one unit. Speaker/namebox names have special handling. `truncated` reports candidates dropped by the term cap. A finite sample can miss terms.

For a normal pipeline, `run --glossary` forces construction and `run --no-glossary` suppresses configured construction. They conflict. Glossary generation is off by default. Existing active terms can still be used in translation even when new construction is off.

Import or export JSON:

```bash
attx glossary import --workspace "./book workspace" --file "./terms.json"
attx glossary export --workspace "./book workspace" --file "./terms export.json"
attx glossary remove --workspace "./book workspace" --src "May"
```

Import accepts `[{"src":"...","dst":"...","info":"..."}]` or an object mapping source strings to target strings. `glossary check` is a substring-based review aid; an inflected translation can trigger a warning without being wrong. Editing a term does not automatically rerun all completed translations.

### Glossary TOML

`<workspace>/glossary.toml` has this schema:

```toml
version = 1
source_lang = "ja"
target_lang = "zh"

[[term]]
src = "アレイ"
dst = "艾蕾"
info = "female character"
count = 12
status = "active"
source = "manual"
case_sensitive = false
```

`src` is required. `dst`, `info`, and `source` default to empty strings, `count` to 0, `case_sensitive` to false, and `status` to `active`. `rejected` terms are retained to avoid repeatedly asking about rejected candidates. `glossary list --all` includes them. The version defaults to 1 and missing language labels default to empty strings. Prefer CLI edits to avoid malformed state.

## Preserve literals and variables

Protected spans become `[CTRL_n]` before the model sees them, then return to their original form. Core rules cover RPG Maker controls, brace placeholders, printf formats, and literal mask tokens. Ren'Py adds interpolation; applicable markup adapters protect inline tags. Validation checks exact multiplicity and relative order for engine controls, markup and implicit positional arguments; named or indexed placeholders can reorder.

```bash
attx preserve list --workspace "./book workspace"
attx preserve add --workspace "./book workspace" --pattern '\bSKU-[0-9]+\b' --info "catalog identifier"
attx preserve remove --workspace "./book workspace" --pattern '\bSKU-[0-9]+\b'
```

Shell single quotes are suitable for these regexes in Bash and PowerShell. Patterns use Rust regex syntax; malformed patterns and empty matches are rejected. Protect actual machine literals, not every source-language word, since overbroad rules can conceal untranslated prose.

### Preserve TOML

`<workspace>/preserve.toml`:

```toml
version = 1

[[rule]]
pattern = '\bSKU-[0-9]+\b'
info = "catalog identifier"
```

`version` defaults to 1; `rule` defaults to an empty array. Each rule requires a pattern and has optional `info` defaulting to empty. Built-ins still apply without this file and cannot be removed through workspace rule removal. Overlapping matches use the leftmost-longest span. Malformed manually edited files/rules are reported and ignored, so check `preserve list` after an edit.

## Translation notes and extraction experience

A glossary specifies term mappings. A prompt note specifies a concrete style requirement:

```bash
attx learn note --workspace "./book workspace" --name voice --text "Keep this narrator's sentences formal; use colloquial dialogue for other speakers."
attx learn list --workspace "./book workspace"
attx learn forget --workspace "./book workspace" --name voice
```

The default `--topic prompt` reaches subsequent translation requests. Other topics are records for humans and agents, not model instructions. Named notes replace their previous value; different names accumulate. Notes do not invalidate completed cache entries.

For an explicitly global format habit:

```bash
attx learn note --format rmmz --name honorifics --text "Keep Japanese honorifics attached to character names."
attx learn list --format rmmz
```

This affects future projects of that format. Use workspace scope unless the broader effect is intended. Do not confuse a style note with a term translation.

After actual writeback, ordinary automatic summarization uses existing evidence and makes no model request. Disable it once with `writeback --no-learn`, or through `[learn].auto_summarize=false`.

```bash
attx learn summarize --workspace "./book workspace"
attx learn pending
attx learn review --approve 1,3 --reject 2
attx learn defaults --format rmmz
attx learn forget --format rmmz --field "resourcepath"
```

`learn scan` is an alias for `summarize`. `--llm` adds paid proposal review. Approval indices are 1-based and refer to the current pending list; reread it before approving. The CLI also offers `--approve-all`, but agents must not approve destructive skip proposals on their own.

Experience is a pure filter over units the adapter already extracted. An `extract` entry cannot create text units that the adapter never produced. Machine-literal safeguards remain. Diagnose missing text with [profiles](profiles.md) or adapter development instead of promising that learning can translate an unknown grammar.

### Experience TOML

Workspace notes and overrides use `experience.toml`; global experience uses `<state-root>/knowledge/<format>.toml`. Current schema:

```toml
format = "rmmz"
version = 2

[[entry]]
kind = "field"
field = "resourcepath"
verdict = "skip"
scope = "nested"
status = "pending"
domain = "plugins"
confidence = 0.95
reason = "These values refer to image resources, not visible text."
evidence = ["Reviewed concrete source examples"]
source = "manual-review"
updated_at = ""

[[entry]]
kind = "note"
topic = "prompt"
text = "Use concise menu labels."
status = "approved"
source = "learn:agent:menus"
updated_at = ""
```

Field entries use lowercased `field`; a leading `*` matches a suffix. `verdict` is `skip` or `extract`, `scope` is `any`, `top`, or `nested`, and `status` is `approved` or `pending`. Empty `domain` applies to all domains. Confidence, reason, evidence, source, and timestamp record provenance, not permission to activate a destructive rule. Notes have `topic`, `text`, `status`, `source`, and `updated_at`; the CLI's named-note key is represented by its source identity.

The active merge is built-in defaults, global experience, then workspace experience. A later layer takes precedence. Within one layer, exact fields beat suffix rules and skip beats extract on a tie. Only approved fields and approved nonempty prompt notes act. Unknown entry kinds/fields survive serialization but do not acquire executable behavior. Older version-1 `[[rule]]` experience is read as approved compatibility data.

For a diagnosed filtering issue:

```bash
attx extract --workspace "./book workspace" --no-knowledge
```

This skips learned filtering for that extraction. It does not fix an adapter that never selected the missing text.

## Manual review and unknown inputs

Export full records with `export-jsonl --filter all`, edit translation fields, and import with `import-jsonl`. Keep source IDs and text unchanged. The import validates the entire file before committing; [workspaces and JSONL](workspace.md) gives the exact fidelity rules.

Use `analyze`, local Auto, and [custom profiles](profiles.md) for unfamiliar formats. `run` can infer a declarative profile in bounded attempts; it never executes model-generated extractors. Binary, encrypted, or ambiguous structures need an external extractor and safe writer.

Continue with [quality and repair](quality.md) or the [CLI reference](cli.md).