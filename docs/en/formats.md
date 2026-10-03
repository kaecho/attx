# Formats

`attx formats` is the runtime inventory. It lists adapter IDs, labels, extensions, input kind, and saved profiles. Detection tries dedicated adapters first, saved profiles next, and local Auto content detection last. A matching extension is not a proof that every document feature is supported.

## Dedicated adapters

| ID | Input | Main output |
|---|---|---|
| `rmmz` | RPG Maker MV/MZ directory | Live `data/*.json` and `js/plugins.js`, backed up |
| `epub` | `.epub` | `<stem>.<dst>.epub` |
| `html` | `.html`, `.htm`, `.xhtml` | Translated sibling |
| `docx` | `.docx` | `<stem>.<dst>.docx` |
| `xlsx` | `.xlsx`, `.xlsm` | Translated sibling |
| `srt` | `.srt` | Translated sibling |
| `vtt` | `.vtt` | Translated sibling |
| `ass` | `.ass`, `.ssa` | Translated sibling |
| `lrc` | `.lrc` | Translated sibling |
| `csv` | `.csv`, `.tsv` | Translated sibling |
| `po` | `.po`, `.pot` | Translated sibling |
| `renpy` | `.rpy` | Translated sibling |
| `md` | `.md`, `.markdown` | `<stem>.<dst>.md` |
| `txt` | `.txt` | `<stem>.<dst>.txt` |
| `paratranz` | Recognized `.json` export | Translated sibling |
| `vnt` | Recognized `.json` VNTextPatch records | Translated sibling |
| `mtool` | Recognized `.json` translation map | Translated sibling |
| `i18next` | Nested `.json` string resources | Translated sibling |
| `jsonl` | `.jsonl` or directory containing `source.jsonl` | File sibling or directory `translated.jsonl` |
| `auto` | Content-recognized file or directory | File sibling or `<input>/translated-<dst>/` |
| `custom:<name>` | TOML profile-selected files | Siblings, or in place if explicitly authored so |

RMMZ is a project adapter, not a general directory dispatcher. JSON adapters sniff structures in priority order. An i18next match can claim a generic nested JSON object; force another adapter only when its semantics actually fit:

```bash
attx init --input "./resources.json" --engine auto --src en --dst zh
```

`detect` itself has no `--engine`; force through `init` or `run`. Forcing does not disable validation or make an incompatible input safe.

## What each family translates

### Books, web pages, and office documents

- EPUB extracts paragraph-level leaf blocks such as paragraphs, headings, and list items. Ruby readings in `rt`/`rp` are excluded from source text. Writeback keeps archive resources and updates `dc:language`; inline styling can be consolidated when text is replaced. It is not a PDF translator.
- HTML translates selected character data while retaining markup. Script and style content should not become ordinary prose. Arbitrary browser behavior or runtime-generated content is outside the adapter.
- DOCX groups text runs into paragraphs, including supported footnote/endnote content. The first text run receives replacement text; fine-grained within-paragraph formatting is not guaranteed to remain distributed as in the original.
- XLSX/XLSM translates `xl/sharedStrings.xml`. Shared values remain consistent across worksheets, and phonetic `rPh` runs are skipped. Inline strings, formulas, chart text, images, and macro behavior are not a promise of complete workbook coverage.

### Subtitles and lyrics

SRT and VTT retain timestamps, headers, and metadata; translation targets cue text. ASS/SSA translates the Text field of `Dialogue:` records, preserves override tags and line-break controls, and uses the Name field as speaker context. LRC translates lyric text without changing timing tags. These are text transformations, not subtitle synchronization, dubbing, or video processing.

### Tables and localization

CSV/TSV creates cell-level units with quoted-field and embedded-newline parsing. Only records affected by selected source text need re-rendering; formatting within those records can change through proper serialization.

PO/POT fills `msgstr` for supported singular entries. The header and plural entries pass through unchanged. It does not implement every gettext plural transformation.

Paratranz targets recognized export records and fills empty translation fields. VNTextPatch uses name/message records. MTool handles its recognized translation mapping. i18next walks nested string values, not keys. These adapters preserve format structures, but a resource key or string that also controls program logic still needs review.

### Scripts, prose, and Markdown

Ren'Py selects dialogue inside translation blocks and `old`/`new` string pairs. It is not a rewrite of arbitrary Python or every `.rpy` statement; voice/play/show/resource statements are excluded.

TXT is line-granular. Markdown skips fenced code and separates heading, quote, and list prefixes from translatable text. Neither is a full parser for every embedded language or every Markdown extension. Source-language heuristics can omit short ambiguous strings.

JSONL is the general interchange route. It does not source-language-filter extraction, since the caller explicitly supplies records. External extraction/writing tools own their original format's semantics.

## Auto is conservative content detection

Auto uses text structure, not a claim that an unknown suffix is supported. It accepts losslessly decoded, confidently parsed subsets:

- Arbitrary JSON string values, with machine-looking keys/values excluded. Byte-span replacements keep unrelated lexical syntax, spacing, key order, and escapes unchanged.
- Strict XML character data. DTDs and XML 1.1 are refused. CDATA is not translated. Attributes are not a general localization surface.
- Scalar subsets of INI, TOML, and simple YAML, including supported quoted forms. This is not a complete YAML processor; advanced tags, anchors, block constructs, or ambiguous syntax can be rejected or left unchanged.
- Confident prose lines, including unknown file extensions or extensionless files. Code-like, binary, control-filled, malformed structured, and lossy-decoded inputs do not fall back to naive line translation.

Machine-looking keys, paths, URLs, identifiers, numeric/boolean values, executable text, and ambiguous literals are kept out of requests. This favors safety over coverage. A Japanese string can still be excluded when it resembles machine data. Inspect extraction instead of assuming everything containing kana was selected.

Auto retains source encoding and BOM. If translated text cannot be represented in a legacy encoding, writeback fails with conversion guidance rather than replacing characters with `?`. Convert a copy to UTF-8 and use a new workspace when appropriate.

### Directory coverage

```bash
attx init --input "./text assets" --engine auto --src ja --dst zh --workspace "./assets workspace"
attx extract --workspace "./assets workspace"
```

Auto output is `text assets/translated-zh/` with the source tree's relative paths. Files it cannot parse are copied unchanged. It does not dispatch each directory member through all dedicated adapters: EPUB/DOCX archives, subtitle syntax, and scripts inside an Auto directory are not automatically translated by their specialized adapters. Run those inputs separately or use an external format-aware workflow.

`extract.auto_coverage` reports:

| Field | Meaning |
|---|---|
| `supported_files` | Files with recognized extractable spans, not a count of successful translations |
| `copied_files` | Files carried unchanged into output |
| `unsupported_total` | Unsupported files in that copy-only coverage |
| `unsupported_paths` | Up to 50 unsupported path samples |
| `excluded_entries` | Excluded tree entries, including directories counted once |
| `excluded_paths` | Up to 50 exclusion samples |

Generated directories, workspaces/cache metadata, backups, and symlinks are excluded; exclusion is different from copying. Language-tagged source assets are retained by copying rather than silently omitted. The coverage report is not a complete per-file translation audit, and sample arrays can be shorter than counts.

## Encodings and structural fidelity

Text decoding tries UTF-8, BOM-marked UTF-16, then a charset guess such as Shift-JIS or GBK. Many dedicated text adapters and custom profiles emit UTF-8. Auto instead verifies encoding roundtrip and retains the source encoding/BOM. ZIP-based office/book formats have their own structured serialization. Do not promise one universal encoding or byte-for-byte output rule across adapters.

No-op Auto replacements preserve source bytes. A custom profile no-op test compares decoded text, not arbitrary original byte encoding; JSON profile no-op renders retain that decoded text exactly, while actual translated values can trigger serialization. Fine formatting can change even when a translated file remains parseable.

## If no adapter matches

`run` can ask the model for a declarative profile in at most three proposal/validation attempts. It refuses to execute generated code and forces copied output. Disable this paid step with `--no-infer`. A safer explicit workflow is [analyze and profile authoring](profiles.md).

Binary/encrypted formats, PDFs, images/OCR, proprietary archives, complex escaped languages, and ambiguous executable content require an external extractor or a real adapter. Use [JSONL interchange](workspace.md) for translation while keeping format-specific safe writeback outside attx.

For RMMZ-specific coverage and layout, read [RPG Maker](rmmz.md). For adding native support, read [development](development.md).