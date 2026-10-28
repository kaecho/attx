# Changelog

## Unreleased

### Changed

- Slot layout is data-driven. A `kind = "layout"` experience entry supplies a leading-label regex, an atomic-control regex, a half-width width budget, a domain, and whether a leading control belongs to the label or the body. Built-in RMMZ rules live in `src/defaults/rmmz.toml`; adapter code no longer owns the bracket table.
- `quality::check_translation` and writeback slot fitting read the active layout rules for any engine. A format that adds a rule gets the same label protection and structural check without a source change.
- `learn layout add|list|remove|test` lets an agent add or probe a rule. `learn summarize` proposes a pending rule when repeated short first slots share delimiters; `learn review` approves it. Existing approved rules suppress duplicate proposals.

## 0.11.2

### Bug fixes

- RMMZ speaker-label protection now recognizes `【】`, `[]`, `〖〗`, `〔〕`, `《》`, `〈〉`, and `［］`, including labels wrapped in colour or expression controls. Writeback keeps the label in slot zero, moves leading body controls back to the body, and never splits inside the label. The 0.8.1 two-slot `【名字】` case remains covered and is now generalized beyond it.
- The RMMZ structural check reads raw message slots instead of masked text, so a label preceded by `\C[2]` or `\SE[...]` is no longer reported as missing. A missing label is still blocking for every supported bracket style.
- Reflow prefers word boundaries, avoids splitting inside a leading label, and never leaves an opening quote at the end of a line.

## 0.11.1

### Changed

- Removed the format-specific speaker-name review. The `review.namebox_mismatch` bucket, the hand-written Japanese shinjitai / Chinese simplified-traditional folding table, the cross-unit name scan and the glossary's namebox exemption are gone. Term and entity consistency is data-driven instead: the glossary reports advisory `glossary.violations`, preserve rules keep literals exact, and a name legitimately rendered as a pronoun is never treated as unwritable.
- `min_occurrences` now applies to every mined glossary term. A rare but important name is added explicitly with `glossary add` instead of bypassing the floor.
- `repair` and writeback share one candidate set: passthrough, structural/quality failure, and target-script residue. Only pending, passthrough and structurally invalid units hold output back.

## 0.11.0

### Bug fixes

- Namebox drift no longer blocks writeback. Advisory speaker-name mismatches stay in `review.namebox_mismatch` and remain `repair` targets, but only pending, passthrough, and structurally invalid units hold output back. The writeback `skipped_note` now reports the exact split (pending / passthrough / failed structural checks) instead of the vague "pending or invalid" wording.
- Namebox comparison joins reflow-split translation slots and folds common Japanese shinjitai and Chinese simplified/traditional variants, so RMMZ message reflow and 汚/污, 桜/樱, 種/种 spelling no longer read as mismatches.

### Translation quality

- Model-emitted line breaks are normalized before structural checks: CRLF/CR collapse to LF, embedded newlines split for long text and arrays, and single-line slots join without inventing separators (Latin word spacing is kept).
- Lines that contain only engine controls, markup, or placeholders are restored from the source when the model drops them and line counts agree.
- Subtitle (SRT/VTT) and LRC writeback preserve the source line ending (CRLF or LF).
- Review adds an advisory `residual_latin` bucket for Latin source fragments left inside a CJK translation. Isolated graphemes and 2–4 uppercase acronyms are exempt, and the bucket never blocks writeback.

### Dynamic extraction rules

- `attx analyze` reports line-shape histograms and ready-to-use JSON text-path globs; directory evidence samples up to three extensions and two files per extension.
- `attx run --infer` and `attx init --infer` force declarative extraction-rule inference for inputs that would fall back to the generic `auto` adapter. A failed inference falls back to `auto` with the error reported in `profile_inference`; `--no-infer` still disables paid inference.
- `profile infer` reports `coverage` (`files_scanned`, `files_with_units`) alongside the validated profile.

## 0.10.1

- Auto source locators and coverage paths use `/` on Windows as well as Unix, preserving the same interchange format and nested-file identities across platforms.
- Release packaging runs the applicable native platform tests before building assets. The unpublished 0.10.0 tag remains unchanged; 0.10.1 contains its full translation, safety, documentation and guided-agent setup changes.

## 0.10.0

### Bug fixes

- RPG Maker message fitting reserves an independent `【speaker】` in slot zero. Dialogue controls remain atomic; punctuation-aware reflow never inserts or deletes event commands. An unavoidable final-slot overflow is retained and reported.
- Chinese-target translations receive conservative mechanical kana cleanup before acceptance, JSONL import and writeback. `っ`/`ッ`, `っすよ`/`っす` and `ー` are normalized only with positive Chinese context. Japanese sentences, semantic glyph examples and protected literals are not mechanically erased; `・` stays intact.
- Actual writeback synchronizes normalized and fitted lines into the cache. Dry runs and blocked writes do not mutate cached translations.

### Translation and automation

- Translation, repair and agents use the same blocking Rust pipeline. Missing or rejected units retry in narrower batches; accepted siblings are retained. Defaults allow two additional targeted repair rounds.
- Requests validate unique accepted IDs, strict choice/short-text structure, exact protected-token multiplicity, control/markup/implicit-argument order, source-script residue and truncated JSON/SSE responses. Named/indexed placeholders may follow target-language grammar.
- Permanent HTTP/configuration failures stop queued and retrying work. Network errors, 408, 429 and server errors use bounded retries. Already committed batches survive later failures.
- Context has a shared per-request character budget and scene/file boundaries. Repair feedback is bounded and treated as data.
- Source extraction order persists for opaque IDs. Accepted rows save before failed siblings finish retries; legitimate unchanged Japanese/Chinese kanji labels do not become forced failures.
- Matching credential strings are redacted from HTTP diagnostics; credential-bearing model content is refused before cache persistence.
- `repair` targets uncapped mechanical findings. Reviews expose separate `kana_edge`, `kana_mixed` and `kana_untranslated` buckets. Glossary substring misses remain advisory.
- The agent protocol no longer requires a separate writeback-permission question for an authorized normal translation. Credentials stay in local configuration, and optional glossary/model-review costs remain explicit.

### Format and writeback safety

- The `auto` adapter content-sniffs JSON, strict XML, a scalar INI/TOML/simple-YAML subset and confident prose. It preserves untouched lexical bytes and representable source encodings. Mixed-directory output copies unsupported assets unchanged and reports unsupported and excluded coverage.
- Unrecognized text can use bounded declarative `profile infer`; `run --no-infer` disables automatic paid inference. Inferred profiles cannot overwrite sources, execute code, accept unsafe quoted captures or skip extraction/roundtrip validation.
- Workspaces bind input, engine, language pair and profile digest. Full source-unit snapshots catch added and removed text. `run` holds a workspace lock through its data pipeline.
- Published-line history and generated-output manifests support safe repeated custom-profile writeback. Live anchors are checked; unrelated live keys/comments are preserved. Source/output collisions fail.
- RPG Maker checks live event, database and plugin anchors against extraction fingerprints. Repeat partial writeback restores unresolved original values instead of leaving an old live translation. Authoritative malformed or symlinked backups fail closed.
- Artifacts stage privately, inherit source permissions and publish through held directory handles. First-write backups preserve source modes. Parent-directory identity changes and backup failures stop publication. Replacement is atomic per file, not across an entire output tree; partial commit errors list committed paths and actual backup availability.
- Pending or invalid units block writeback by default. `--allow-partial` explicitly writes a valid subset and returns `needs_attention`. Incomplete runs and blocked/partial writes emit JSON and exit 2 rather than report complete success.
- JSONL imports validate the complete batch before a transaction: location/hash aliases, exact source text, duplicate-unit conflicts, structure and protected literals. Empty audit records are skipped; invalid imports do not partially update the cache.

### Documentation and release

- Rewrote English and Chinese READMEs and the complete English, Chinese and Japanese manuals. Each locale covers installation, quick start, every configuration field, workflows, formats, profiles, RPG Maker, quality, CLI, agents, workspace schemas, architecture, development and recovery.
- Added a separate agent-guided translation section in all languages, with a complete copy-paste prompt. The agent reads project docs, asks explained API/model/language/budget questions, securely collects credentials and generates configuration; users do not need to edit TOML.
- Releases verify tests, CLI version/smoke and strict documentation builds before packaging Linux, Windows and both macOS architectures. Packages include documentation sources and changelog.
- Rust minimum version is 1.89. Capability-based artifact I/O uses `cap-std`; directory identity checks use `same-file`.
- Translation/agent design draws on inspected LinguaGacha source through an independent Rust implementation. No upstream code was copied.

### Migration

Keep existing workspaces and backups. Run `extract --workspace <workspace>` to refresh source/anchor snapshots; matching source hashes retain cached translations. Then use `repair`, `review` and `writeback`. Do not modify cached Profile files to change output policy; use a new workspace for different rules or language pairs. Check automation for exit 2 and the new status/report fields. Glossary generation remains off by default.

Structural checks cannot prove linguistic accuracy or game rendering. Unknown binary/encrypted formats still require a format-aware extractor and writer; a successful no-op profile roundtrip does not establish complete extraction coverage.
