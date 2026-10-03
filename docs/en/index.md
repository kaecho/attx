# attx

attx (Agent Translation Toolkit eXtensible) is a local translation CLI for humans and coding agents. Version 0.10.0 is a single Rust binary. It extracts text through format adapters, translates through an OpenAI-compatible Chat Completions endpoint, caches results in SQLite, and renders translated artifacts.

```text
input -> adapter -> text units -> cached translation and bounded repair -> adapter -> output
```

Supported inputs include RPG Maker MV/MZ projects, EPUB, HTML, Word and Excel documents, subtitles, plain text, Markdown, localization files, and declarative custom profiles. The Auto adapter also handles conservative subsets of structured text with unfamiliar extensions. It does not make every file safe to translate.

## Start here

If you want to answer questions rather than install and configure the tool yourself, start with [Translate with an agent](agent-translation.md). Copy one prompt; a capable coding agent reads the docs, handles secure credential collection, writes the settings, and runs the translation. Each question explains the setting and a recommendation. attx itself does not have an interactive setup command.

Prefer to run the commands yourself? Follow the manual path:

1. [Install](install.md) the binary or build with Rust 1.89 or newer.
2. Follow the [quick start](quickstart.md) to configure an endpoint and produce your first translated file.
3. Read [configuration](configuration.md) before increasing concurrency or enabling optional paid requests.
4. Use [workflows](usage.md) for incremental work, manual review, and terminology.

```bash
attx --config "./setting.toml" doctor --ping --json
attx --config "./setting.toml" run --input "./novel.epub" --src ja --dst zh
```

An EPUB input normally produces `novel.zh.epub` beside the source. An RPG Maker project writes into its live data directory with checked backups. [Formats](formats.md) explains these differences.

## What completion means

A request can finish with unresolved units. attx reports pending text, source-text placeholders, residual source script, lost protected tokens, name inconsistencies, and layout overflow. Translation and repair are bounded; they do not guarantee literary quality, complete format coverage, or a playable game.

Default writeback blocks incomplete or invalid translations. A deliberate `--allow-partial` writes the valid subset and retains originals for the rest, with exit code 2. `--dry-run` produces a plan, not a translated artifact. Read [quality and repair](quality.md) and [CLI results](cli.md) before automating completion checks.

Completed cache entries survive interruption. Resume with the same workspace and language pair instead of deleting the database. Resumption can still spend requests on unresolved units or newly changed source text. See [workspaces and JSONL](workspace.md).

## Manual map

| Task | Page |
|---|---|
| Install binaries and toolchains | [Installation](install.md) |
| Guided setup and translation by a coding agent | [Translate with an agent](agent-translation.md) |
| First runnable translation | [Quick start](quickstart.md) |
| Every configuration key and default | [Configuration](configuration.md) |
| Run, resume, glossary, preserve, and learning | [Workflows](usage.md) |
| Supported formats and Auto limits | [Formats](formats.md) |
| Unknown formats and declarative rules | [Custom profiles](profiles.md) |
| Game data, plugins, and fixed dialogue slots | [RPG Maker MV/MZ](rmmz.md) |
| Mechanical checks and bounded repair | [Quality and repair](quality.md) |
| All commands, flags, reports, and exit codes | [CLI reference](cli.md) |
| Unattended execution and agent boundaries | [Agents](agents.md) |
| Cache identity, backups, locks, and interchange | [Workspaces and JSONL](workspace.md) |
| Technical dataflow and component boundaries | [Architecture](architecture.md) |
| Adapter development, tests, docs, and release | [Development](development.md) |
| Symptoms and recovery steps | [Troubleshooting](troubleshooting.md) |

## Privacy and authorization

Translation sends selected source text, bounded context, relevant glossary terms, and prompt notes to your configured endpoint. Paid glossary generation, model-reviewed learning, and unknown-format inference can send additional samples. The API key stays in local configuration and HTTP authentication, not the workspace database. Do not publish private configuration or exported text.

A request to translate a named input authorizes its normal pipeline and writeback under the bundled agent protocol. There is no mandatory second writeback permission prompt. This does not authorize deleting workspaces, modifying unrelated inputs, enabling unrequested paid features, or silently accepting partial output.

Source: [GitHub repository](https://github.com/kaecho/attx). License: [MIT](https://github.com/kaecho/attx/blob/main/LICENSE).