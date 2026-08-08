# Formats

| id | Input | Output |
|----|-------|--------|
| `epub` | `.epub` | `<name>.<dst>.epub` |
| `html` / `docx` / `xlsx` | file | translated copy |
| `txt` / `md` | text | `<name>.<dst>.*` |
| `srt` / `vtt` / `ass` / `lrc` | subtitles | translated copy |
| `csv` / `po` / `renpy` | file | translated copy |
| `rmmz` | game directory | in-place + `*.attxbak` |
| `mtool` / `paratranz` / `vnt` / `i18next` | `.json` (sniffed) | translated copy |
| `jsonl` / `custom:<name>` | file/dir | escape hatch / profile |

```bash
attx formats
attx detect --input <path>
```

## Unknown format

```bash
attx analyze --input ./project
attx profile new --output fmt.toml
attx profile test --profile fmt.toml --input ./project --roundtrip
attx init --input ./project --profile fmt.toml --src ja --dst zh
attx profile save --profile fmt.toml
```

See `profiles/examples/` and `skills/attx/references/custom-format-discovery.md`.
