# Architecture

attx is a binary crate with synchronous control flow. It uses blocking HTTP, scoped worker threads, a SQLite workspace, declarative format profiles, and pure format adapters. There is no asynchronous runtime, daemon, dynamic executable plugin system, or built-in MCP server.

## Dataflow

```text
CLI arguments + settings
        |
        v
adapter selection: dedicated -> saved profile -> Auto
        |                         |
        |                   unmatched run input
        |                         v
        |              bounded profile proposal/validation
        v
workspace identity + operation lock
        |
        v
extract full source snapshot -> learned pure filter -> units in SQLite
        |
        v
pending/flagged selection -> batching + masks + bounded neighbor context
        |
        v
blocking Chat Completions workers -> parse/quality validation -> batch cache commits
        |
        v
mechanical review -> bounded targeted repair passes
        |
        v
normalize + format layout + completeness + current source/anchor validation
        |
        v
adapter renders artifacts in memory
        |
        v
stage all artifacts -> check backups -> atomic replacement per file
        |
        v
persist written normalization/layout + published lines -> optional experience summary
```

A dry-run stops before model or durable translation/artifact changes as appropriate for that command. A blocked writeback returns a structured report before artifact rendering/replacement. Standalone JSONL shares the translator but has no workspace repair/resumption layer.

## Component map

| Module | Responsibility |
|---|---|
| `main.rs` | Clap command definitions, dispatch, final stdout/exit status |
| `config.rs` | TOML schema, defaults, config resolution, client selection |
| `pipeline.rs` | Detection order, workspace identity, extraction, translation/repair orchestration, writeback, analysis, JSONL exchange |
| `model.rs` | `TextUnit`, `Translation`, workspace metadata, IDs/source hashes, script heuristics |
| `store.rs` | SQLite source units/translations/meta and published-line state, transactional saves |
| `llm.rs` | Prompts, format registers, batching, neighbor context, limiter, blocking HTTP, JSON/SSE decoding, bounded retries |
| `preserve.rs` | Literal/variable/control masking and restoration, multiplicity/order checks |
| `quality.rs` | Unit structure, target normalization, script residue, protected-content checks |
| `review.rs` | Mechanical findings, bounded report samples, complete repair-candidate selection |
| `glossary.rs` | Local terminology, bounded paid extraction, relevance injection, advisory application checks |
| `knowledge.rs` | Layered experience schema, approved notes, pure extraction filter |
| `learn.rs` | Evidence summaries, pending approval, notes and forget operations |
| `profile.rs` | Declarative custom adapter compilation, selection, source/live anchor validation and safe render |
| `fileio.rs` | Workspace OS locks, directory-handle-anchored staging/backup/replacement |
| `textio.rs` | Encoding detection/decoding |
| `adapter/` | Format-specific extraction and rendering only |

Source: [src directory](https://github.com/kaecho/attx/tree/main/src). The built-in registry lives in [adapter/mod.rs](https://github.com/kaecho/attx/blob/main/src/adapter/mod.rs).

## Format boundary

`FormatAdapter: Send + Sync` exposes stable ID/label/extensions/input kind, optional detection, `extract(input, source_lang)`, and `writeback(input, target_lang, units, translations)`. Extraction returns engine-neutral units. Writeback returns `OutputFile` values containing destination path, bytes, and optional source permissions; it does not commit the files itself.

Adapters perform I/O and structure logic but never make network requests. The pipeline owns model calls, cache writes, permission-aware staging, backups, and final replacement. New formats therefore share the same translation/review mechanisms rather than inventing a parallel model client.

`TextUnit` contains ID, engine, domain, location, item type, role, original lines, optional per-line paths, context grouping, and format payload. Common IDs are truncated SHA-256 derived from engine/location/source lines; source hashes track original line content. Locations are human-readable interchange identities, not arbitrary write paths trusted from a model.

## Workspace state

SQLite uses WAL and tables for metadata, units, translations, and actually published lines. Original line arrays and payloads are serialized values; translations store source hashes and explicit passthrough state. Source extraction replacement and JSONL translation import use transactions. HTTP work commits accepted batches incrementally, not only when the entire project succeeds.

Extraction stores an `ordinal` for each unit. Later batch and neighbor selection retains source order, including opaque JSONL IDs; natural numeric locators are ordered within their scene. Accepted rows reach the caller's cache sink before failed siblings finish retries. Configured credentials are never sent as translation content; credential-bearing model content is refused, and matching HTTP error strings are redacted.

Initialization binds canonical content root, engine, normalized language pair, and copied profile digest. A complete extraction snapshot is retained before learned filtering so source additions/removals cannot hide behind cached units. Changed source/profile identity is not repaired by silently changing workspace metadata.

Mutating commands acquire an OS lock; `run` holds it across its stages. The lock releases on process failure. This prevents two cooperating CLI commands from racing one workspace, not all possible external edits to an input or a provider's global account quota.

Published lines record what attx actually wrote. Custom in-place anchor checks accept original text, the current translation, or a previously published translation at a selected location. Rendering starts from the live document, so unrelated live keys/comments remain instead of being overwritten by a backup copy. Backup text remains the extraction reference.

## Requests and failure handling

Batches obey source-character and item-count limits and format/context grouping. Neighbor context uses a separate total-character budget, includes bounded previous/next source or committed translation data, and follows natural location order within scene/file boundaries.

The system prompt selects game, literary, subtitle, document, or software wording based on the adapter. It requires structured IDs/line arrays, protected controls, speaker/name consistency, and no context echoing. Approved prompt notes and relevant glossary terms add constraints. Input/context/previous-output fields are data, not trusted instructions.

Workers share a request-start limiter within one translator. Recoverable transport or HTTP 408/429/5xx failures retry within a per-unit attempt budget. Format/quality failures retain accepted IDs and narrow only failed candidates. Permanent HTTP 4xx stops queued work; already in-flight requests may finish. There is no key rotation, AIMD concurrency controller, provider fallback, or indefinite agent retry engine.

Response decoding checks ordinary Chat Completions JSON or SSE content, rejects truncation and malformed streams, discards unknown IDs, and counts accepted IDs once. Structural validation and narrow Chinese-target normalization precede acceptance. Workspace review/repair makes finite additional passes; source placeholders remain visible after exhaustion.

## Writeback guarantees and limits

The pipeline normalizes cached Chinese text and applies RMMZ fixed-slot layout in memory, then computes the valid subset. Default incomplete output is blocked. Deliberately allowed partial output keeps/restores source values for invalid units and reports incomplete status.

Before artifact replacement, the current source set, source hashes, and format anchors must match the extraction reference. Auto uses file-byte hashes and span/style anchors. Custom profiles validate selected live text, line prefix/suffix anchors, and JSON array ancestor lengths to reject index shifts. RMMZ also checks structural projections of event topology, codes/indent/nontext parameters, name/page relationships, database identities, and plugin identity/nesting.

Safe unrelated map coordinates/settings, gameplay values, and plugin descriptions can remain live. Conservative structural changes such as page conditions, event nontext parameters, or plugin identity parameter changes require extraction refresh. Structurally indistinguishable text-only slot swaps cannot always be identified by a text-excluding projection. These checks reduce stale writes; they are not a proof of all engine semantics.

All output bytes are staged before replacements begin, and all required existing-file backups must be valid before the batch proceeds. Backup names append `.attxbak` and preserve the first destination. Staging/backup/replacement uses open directory capabilities via `cap-std` 4 and parent identity handles via `same-file` 1. Temporary files start private; completed backups and outputs inherit source permissions. Symlink outputs and unsafe backup objects are refused.

Replacement is atomic per file, not an entire-directory transaction. A late failure can leave earlier files replaced. The cache records normalized/reflowed/published text only after actual successful writeback; dry-run/blocked paths do not persist candidate text. A private staging file does not make a later intentionally readable output private.

## Auto versus custom profiles

Auto is a conservative built-in parser for lossless JSON spans, strict XML character data, simple scalar configurations, and confident prose. It retains source encoding/BOM and copies unsupported directory files unchanged, with separate excluded-entry coverage. It is not a directory dispatcher for every dedicated format.

Custom profiles are declarative selection rules, not executable programs. Inference proposes at most three times, forces `overwrite=false`, and checks extraction/machine literals/source no-op roundtrip. ASCII-quoted/backtick code captures and programming escapes are unsafe without an escaping strategy and are rejected. Target newline/delimiter corruption is rejected during rendering. A no-op profile validation still cannot prove natural-language correctness or all target-syntax semantics.

## Independent design credit

Selected reliability ideas were studied in [LinguaGacha](https://github.com/neavo/LinguaGacha) at commit `5a0058d57abfc6df371339818894097f3879bd28` (v0.125.0): bounded failed-unit retry, duplicate/omission rejection, line-count/protected-content checks, foreign-script review, bounded context, shared batch/agent contracts, and staged sources.

Code evidence includes its [batch runner](https://github.com/neavo/LinguaGacha/blob/5a0058d57abfc6df371339818894097f3879bd28/src/backend/batch-translation/core/batch-translation-runner.ts), [response decoder](https://github.com/neavo/LinguaGacha/blob/5a0058d57abfc6df371339818894097f3879bd28/src/backend/batch-translation/work-unit/response/response-decoder.ts), [proofreading evaluator](https://github.com/neavo/LinguaGacha/blob/5a0058d57abfc6df371339818894097f3879bd28/src/shared/proofreading/proofreading-evaluator.ts), and [agent runtime document](https://github.com/neavo/LinguaGacha/blob/5a0058d57abfc6df371339818894097f3879bd28/docs/AGENT_RUNTIME.md).

attx implements these concepts independently in Rust. It does not copy upstream code, adopt the Electron runtime, or assert license compatibility. The inspected [English README](https://github.com/neavo/LinguaGacha/blob/5a0058d57abfc6df371339818894097f3879bd28/README_EN.md) requires attribution and commercial authorization; no root license was observed in that inspection. attx's Auto/inference extension is its own design, not a claim that upstream automatically supports every unknown format.

Continue with [development](development.md) for implementation examples and test strategy.