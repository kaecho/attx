# Changelog

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
