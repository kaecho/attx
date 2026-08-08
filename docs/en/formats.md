# Formats

## Built-in adapters

`attx formats` prints the authoritative list as JSON — ids, extensions, and whether the input is a file or directory. Detection order is fixed; the JSON `.json` flavors are sniffed by content, most specific first.

| id | Extensions | Input | Output |
|----|-----------|-------|--------|
| `rmmz` | — | directory | in-place + `*.attxbak` |
| `epub` | `.epub` | file | `<name>.<dst>.epub` |
| `html` | `.html` `.htm` `.xhtml` | file | translated copy |
| `docx` | `.docx` | file | `<name>.<dst>.docx` |
| `xlsx` | `.xlsx` `.xlsm` | file | translated copy |
| `srt` | `.srt` | file | translated copy |
| `vtt` | `.vtt` | file | translated copy |
| `ass` | `.ass` `.ssa` | file | translated copy |
| `lrc` | `.lrc` | file | translated copy |
| `csv` | `.csv` `.tsv` | file | translated copy |
| `po` | `.po` `.pot` | file | translated copy |
| `renpy` | `.rpy` | file | translated copy |
| `md` | `.md` `.markdown` | file | translated copy |
| `txt` | `.txt` | file | translated copy |
| `paratranz` | `.json` (sniffed) | file | translated copy |
| `vnt` | `.json` (sniffed) | file | translated copy |
| `mtool` | `.json` (sniffed) | file | translated copy |
| `i18next` | `.json` (sniffed) | file | translated copy |
| `jsonl` | `.jsonl`, or dir with `source.jsonl` | file or dir | `translated.jsonl` |
| `custom:<name>` | from profile | file or dir | copy, or in-place if `overwrite = true` |

Force a specific adapter when detection is ambiguous or wrong: `attx init --engine <id>`.

### Per-format notes

- **epub** — paragraph-level units over leaf blocks (`p`, headings, `li`, …); ruby readings (`<rt>`/`<rp>`) are stripped from the source text; images and layout preserved; `dc:language` updated on writeback.
- **docx** — paragraph-level over `w:t` runs (body + footnotes/endnotes); the first run of each paragraph receives the translation.
- **xlsx** — translates the shared-string table (`xl/sharedStrings.xml`), so every sheet stays consistent; phonetic `rPh` runs are skipped.
- **srt/vtt/lrc** — timing lines, headers and metadata stay verbatim; only cue/lyric text is translated.
- **ass** — only the `Dialogue:` Text field; `{\tag}` overrides and `\N` line breaks are preserved; the `Name` column becomes the speaker role.
- **csv/tsv** — per-cell units (RFC 4180: quoted fields, embedded newlines); only records with source-language text are re-rendered.
- **po** — fills `msgstr`; the header entry and `msgid_plural` entries pass through untouched.
- **renpy** — only inside `translate` blocks: quoted dialogue plus `old`/`new` string pairs; asset statements (voice/play/show/…) are skipped.
- **rmmz** — see [RPG Maker](rmmz.md).
- **mtool/paratranz/vnt/i18next** — content-sniffed JSON shapes (MTool `ManualTransFile.json`, Paratranz export filling only empty `translation` fields, VNTextPatch `name`/`message`, i18next nested string leaves).
- **jsonl** — the escape hatch: any engine via external extract/write scripts; no source-language filtering on extract.

### Encoding

Text inputs auto-detect encoding: strict UTF-8 → UTF-16 (BOM) → `chardetng` guess (Shift-JIS, GBK, …) → `encoding_rs` decode. Output is **always UTF-8**.

## Unknown format? Teach attx a profile

```bash
attx analyze --input ./project         # recon: encoding, structure, samples, JSON shape
attx profile new --output fmt.toml     # documented rule template
attx profile test --profile fmt.toml --input ./project --roundtrip   # iterate
attx init --input ./project --profile fmt.toml --src ja --dst zh
attx profile save --profile fmt.toml   # detect auto-recognizes it from now on
```

### Profile schema

```toml
name = "myformat"                    # id → engine "custom:myformat"
label = "My format"
extensions = ["ks"]                  # e.g. ["ks", "scn"]
detect_regex = []                    # ALL must match in the first 64 KiB
min_units = 1                        # auto-detect needs ≥ this many units
overwrite = false                    # true = write back in place
skip_lines = []                      # line_regex mode: skip matching lines
notes = ""

# Per-line regex: (?P<text>...) required, (?P<role>...) optional
[[rules]]
kind = "line_regex"
pattern = '^(?P<role>[^\s@;]*)\s*「(?P<text>.+)」$'

# JSON: string values under these object keys (any depth)
[[rules]]
kind = "json_keys"
keys = ["message", "name"]

# JSON: string leaves at path globs (* one level, ** any depth)
[[rules]]
kind = "json_paths"
paths = ["events/*/text", "**/choices/*"]
```

Saved profiles live in `$ATTX_HOME/profiles/` (or `~/.config/attx/profiles/`) and appear in `attx formats` / `attx detect` as `custom:<name>`.

Examples: `profiles/examples/` (KiriKiri KAG, INI, generic JSON). Agent workflow: `skills/attx/references/custom-format-discovery.md`.
