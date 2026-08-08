# Formats

| id | Input | Output |
|----|-------|--------|
| `rmmz` | game directory | in-place + `*.attxbak` |
| `epub` | `.epub` | `<name>.<dst>.epub` |
| `html` | `.html` `.htm` `.xhtml` | translated copy |
| `docx` | `.docx` | `<name>.<dst>.docx` |
| `xlsx` | `.xlsx` `.xlsm` | translated copy |
| `txt` / `md` | text | `<name>.<dst>.*` |
| `srt` / `vtt` / `ass` / `lrc` | subtitles | translated copy |
| `csv` | `.csv` `.tsv` | translated copy |
| `po` | `.po` `.pot` | fills `msgstr` |
| `renpy` | `.rpy` | translated copy |
| `mtool` / `paratranz` / `vnt` / `i18next` | `.json` (sniffed) | translated copy |
| `jsonl` | file/dir | `translated.jsonl` |
| `custom:<name>` | per profile | copy or in-place |

```bash
attx formats
attx detect --input <path>
```

## Unknown format

```bash
attx analyze --input ./game
attx profile new --output fmt.toml
attx profile test --profile fmt.toml --input ./game --roundtrip
attx init --input ./game --profile fmt.toml --src ja --dst zh
attx profile save --profile fmt.toml
```

See `profiles/examples/` and `skills/attx/references/custom-format-discovery.md`.
