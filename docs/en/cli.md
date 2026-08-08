# CLI

| Command | Role |
|---------|------|
| `doctor [--ping] [--json]` | Config / LLM ping |
| `formats` | Adapters + saved profiles (JSON) |
| `detect --input <path>` | Format probe (`--game` alias) |
| `analyze --input <path>` | Recon for unknown inputs |
| `profile new/test/save/list` | Custom format profiles |
| `init --input --src --dst [--profile]` | Workspace + SQLite |
| `extract --workspace` | Units into DB |
| `translate --workspace [--limit] [--dry-run] [--retry-passthrough]` | LLM over pending |
| `writeback --workspace [--dry-run] [--no-learn]` | Render output |
| `run --input …` | init + extract + translate + writeback |
| `status --workspace` | Counts + domains |
| `export-jsonl` / `import-jsonl` / `translate-jsonl` | Interchange |
| `learn …` | Experience layer |
| `glossary …` | Proper-noun glossary |

Global flags: `--config`, `--client <name>`.
