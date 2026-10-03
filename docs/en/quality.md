# Quality and repair

attx validates mechanical structure and exposes suspicious translations. These checks do not prove semantic accuracy, natural style, complete extraction, or correct runtime behavior. Read both the translation/writeback report and any format-specific coverage or layout fields.

## Unit contracts

Text units carry an item type:

| Type | Output constraint |
|---|---|
| `short_text` | Exactly one translation string |
| `array` | Exactly the original number of lines |
| `long_text` | Can reflow according to the format's allowed prose contract |

Empty/structurally invalid output, missing or altered protected literals, wrong mask multiplicity/order, invalid IDs, and truncation are failures. The model must return the requested units, not adjacent context. Unknown IDs are discarded; duplicate IDs never increase success counts. Accepted IDs remain accepted once; omitted or failed units are the retry candidates.

Protected spans include engine controls, variables, formatting tokens, and configured literals. All must retain their multiplicity. Engine controls, markup and implicit positional printf arguments also retain relative order; named or explicitly indexed placeholders may move with target-language grammar. Mere presence of `%s` somewhere does not prove its original argument survived correctly.

## Mechanical review

```bash
attx review --workspace "./workspace"
```

This uses no HTTP. It returns overall counts and these buckets, each containing `count` plus up to 40 samples of `{location, unit_id, detail}`:

| Bucket | Signal |
|---|---|
| `residual_source` | Aggregate source-script residue, including relevant identical/passthrough originals |
| `kana_edge` | Mechanical Japanese kana fragments around otherwise convincing Chinese text |
| `kana_mixed` | Japanese kana mixed into target-language text |
| `kana_untranslated` | Japanese passages that still need translation |
| `identical` | Source text copied unchanged where translation is needed |
| `control_loss` | Missing, changed, reordered, or incorrectly repeated protected tokens |
| `namebox_mismatch` | Speaker/name relationship inconsistent between namebox and body |

The three kana categories are disjoint classifications. Other buckets can overlap the aggregate or each other, so summing counts is not a unique unresolved-unit total. `review.glossary` has active/seen/applied term counts and `violations`; exact target-substring misses are advisory and can be false positives for inflected languages.

`status.pending=0` does not imply clean translations. Passthrough is separate from successful translation; it is a source placeholder after failed/refused attempts. `status` measures cache progress, while `review` adds quality signals. Sample caps do not cap the repair candidate set. Use `export-jsonl --filter all` for full records.

Source-language detection is a script heuristic covering common Latin, CJK, Hangul, Cyrillic, Arabic, Devanagari, and Thai ranges. Shared scripts, short strings, names, and same-script translation pairs limit its accuracy. Tags select prompts and heuristics; they are not an automatic language-identification service.

## Chinese target cleanup

Chinese-target normalization is deliberately narrow. In convincing Chinese context it can remove isolated `っ`/`ッ`, map colloquial `っすよ`/`っす` residue to `哦`, and map a Japanese prolonged-sound mark `ー` to `～`. It keeps separators such as `・`, protected literals, genuine quoted-glyph semantics, and full Japanese content that still requires translation.

This normalization is shared by model acceptance, JSONL import, and existing-cache handling. It does not delete complete Japanese sentences to make residue counters look clean. Mixed/untranslated content remains visible for repair. Normalized cache changes during actual translation/import are persisted; writeback's in-memory normalization/reflow is persisted only after actual artifact replacement. Dry-run and blocked writeback do not modify translations.

## Retry is not the same as repair

Within one translator pass, each unit has at most `1 + retry_count` attempts. Recoverable network errors, HTTP 408, 429, and 5xx retry within the configured budget. Model format/quality failures retry only rejected units in narrower batches; successful units are not resent because a neighbor failed.

Permanent HTTP 4xx errors such as authentication, invalid endpoint/model, or unsupported request parameters stop queued work. attx does not rotate keys or repeatedly split the same unauthorized request. In-flight requests may finish, and already committed batches remain in SQLite.

A workspace translation makes an initial pass plus up to `repair_rounds` extra targeted passes. Explicit `repair` makes at most `max(1, repair_rounds)` passes. Defaults are three retries and two extra repair rounds. These bounds control attempts, not guaranteed success or cost. Separate commands start separate budgets.

```bash
attx --config "./setting.toml" translate --workspace "./workspace"
attx --config "./setting.toml" repair --workspace "./workspace" --dry-run
attx --config "./setting.toml" repair --workspace "./workspace"
```

Both workspace translation and repair use pending/mechanically flagged candidates. `translate --retry-passthrough` explicitly requeues source placeholders. Failed repair results do not replace an existing human/model translation with a new failed placeholder.

The report's `planned`, `translated`, `repaired`, `repair_rounds`, `unresolved`, `pending_after`, and `review` explain what happened. Do not repeatedly rerun a bounded command until the provider stops charging; inspect persistent failures and correct them through JSONL or configuration.

## Writeback completeness

Default writeback refuses unresolved/pending units with `status="blocked"`, no artifact paths, and exit 2. This is a data-integrity condition, not a writeback permission prompt.

```bash
attx writeback --workspace "./workspace" --dry-run
attx writeback --workspace "./workspace"
```

Dry-run returns a plan or blocked report with exit 0 and never writes output or normalized cache text. An explicit partial write:

```bash
attx writeback --workspace "./workspace" --allow-partial
```

writes only valid units and keeps source text for unresolved ones. It still reports `needs_attention` and exits 2. Do not add the flag automatically to bypass the user's expectation of a complete translation.

RMMZ reflow counts and unavoidable overflow are separate display signals. Zero structural errors does not guarantee that a font, text box, or subtitle line fits. Review and playtest the written artifact.

Kanji-only labels shared by Japanese and Chinese, such as `魔法` or `通信`, are not rejected solely because the text is unchanged. This avoids inventing a failed translation for a valid target label; it also means shared-script semantic correctness still needs review.

## Human correction route

```bash
attx export-jsonl --workspace "./workspace" --output "./review.jsonl" --filter all
# Edit translation_lines or translation in a local editor.
attx import-jsonl --workspace "./workspace" --input "./review.jsonl"
attx review --workspace "./workspace"
attx writeback --workspace "./workspace"
```

Keep `id` and `text` unchanged. Invalid controls or line contracts reject the whole import; semantic residue can remain an explicit repair finding. A glossary mapping or style note affects later requests, not already accepted entries by itself.

For report field meanings, see [CLI](cli.md). For cache/source guarantees and external JSONL extraction, see [workspaces](workspace.md).