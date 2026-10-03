# attx

[中文](README.zh-CN.md) | [Documentation](https://kaecho.github.io/attx/) | [Releases](https://github.com/kaecho/attx/releases)

Agent Translation Toolkit eXtensible is a Rust command-line translator for games, books, documents, subtitles and localization files. It uses an OpenAI-compatible Chat Completions endpoint and stores progress in a SQLite workspace.

```text
identify → extract → optional glossary → translate → check and repair → safe writeback
```

Version 0.10.1 keeps translation and agent automation on the same pipeline. Failed units receive bounded targeted retries; unresolved units remain visible. Normal translation requests no longer require a separate writeback-permission conversation.

## Let an agent handle setup

Copy the complete prompt in [translate with an agent](docs/en/agent-translation.md). The agent reads installation, usage and configuration docs, asks about your API and securely receives the key, explains model/language/budget/parameter choices, then installs, configures and runs attx. You answer questions without editing TOML.

```text
Use https://github.com/kaecho/attx. Read its installation, usage and configuration
docs, skills/attx/SKILL.md and skills/attx/references/agent-setup.md first.
I do not want to configure it manually. Ask me about the translation API,
receive the key through secure input or a hidden interactive terminal, then
ask about model, input, languages, budget and parameters. At every step explain
what it is, what it enables, your recommendation, and cost/risk. Install the tool,
generate private configuration, verify it and perform the translation yourself.
Reuse working settings; never echo credentials or add a second writeback approval.
```

## Install

Download an archive for your platform from [GitHub Releases](https://github.com/kaecho/attx/releases): Linux x86_64, Windows x86_64, macOS Apple Silicon or macOS Intel. Archives contain the binary, example configuration, agent Skill, custom profile examples and documentation sources.

From source, use Rust 1.89 or newer:

```bash
git clone https://github.com/kaecho/attx.git
cd attx
cargo build --release
./target/release/attx --help
# Optional installation into Cargo's bin directory:
cargo install --path . --locked
```

The examples below assume `attx` is on `PATH`. On Windows, use `./attx.exe` or the full executable path.

## First translation

Copy `setting.example.toml` to `setting.toml`. Fill in `base_url`, `api_key` and `model` locally. Do not send the key through agent chat or commit it.

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
base_url = "https://your-provider.example/v1"
api_key = "YOUR_API_KEY"
model = "your-model-name"
```

The endpoint must accept `{base_url}/chat/completions`. This is not a native Anthropic, Gemini or Responses API client. Omitted translation, glossary and learning sections use their defaults.

```bash
attx --config ./setting.toml doctor --ping --json
attx --config ./setting.toml run --input "novel.epub" --src ja --dst zh
```

Check `doctor`'s `llm.configured` and `ping` fields. Its exit code alone does not prove connectivity. `--ping` makes a small paid model request.

The second command writes `novel.zh.epub`; the original stays unchanged. Its workspace is `.attx-novel/` beside the input. A directory normally uses `<input>/.attx/`.

PowerShell:

```powershell
Copy-Item setting.example.toml setting.toml
# Edit setting.toml locally before these commands.
./attx.exe --config ./setting.toml doctor --ping --json
./attx.exe --config ./setting.toml run --input "C:/Books/novel.epub" --src ja --dst zh
```

For a trial without writing output:

```bash
attx run --input "novel.epub" --src ja --dst zh --limit 20 --no-writeback
attx translate --workspace ".attx-novel"
attx writeback --workspace ".attx-novel"
```

A limited trial can exit 2 because other units are still pending. That is a progress report, not a lost cache. Re-running `translate` preserves completed translations.

## Formats and output paths

| Input | Adapter IDs | Output |
|---|---|---|
| RPG Maker MV/MZ directory | `rmmz` | Live `data/*.json` and `js/plugins.js`, with first-write backups |
| EPUB, HTML, Word, Excel | `epub`, `html`, `docx`, `xlsx` | Language-suffixed sibling |
| Plain text, Markdown | `txt`, `md` | Language-suffixed sibling |
| SRT, WebVTT, ASS/SSA, LRC | `srt`, `vtt`, `ass`, `lrc` | Language-suffixed sibling |
| CSV/TSV, gettext PO/POT | `csv`, `po` | Language-suffixed sibling |
| Ren'Py translation exports | `renpy` | Language-suffixed `.rpy` sibling |
| MTool, Paratranz, VNTextPatch, i18next JSON | `mtool`, `paratranz`, `vnt`, `i18next` | Language-suffixed JSON sibling |
| JSONL text packs | `jsonl` | File sibling or directory `translated.jsonl` |
| Content-sniffed structured text | `auto` | File sibling or directory `translated-<dst>/` tree |
| Declarative TOML profile | `custom:<name>` | Sibling by default; optional in-place writeback |

```bash
attx formats
attx detect --input "input.file"
attx analyze --input "input.file" --src ja
```

`auto` recognizes arbitrary JSON string values, strict XML character data, an INI/TOML/simple-YAML scalar subset and confident prose, even with an unknown extension. It protects structural bytes and retains the original encoding when the translated text is representable.

Auto directory mode copies unsupported files unchanged and reports `extract.auto_coverage`. It does not dispatch every built-in document, archive, subtitle or script adapter within that tree. A copied file is not a translated file. Binary containers, encrypted assets, ambiguous scripts and unsupported syntax need a dedicated adapter or external extraction.

For unrecognized textual formats, `run` can ask the configured model for a declarative profile. It validates extraction and a source-preserving no-op roundtrip, forces `overwrite = false` and allows at most three proposals. `--no-infer` disables this extra paid operation. Models can still choose incomplete or semantically wrong fields; validation does not prove complete coverage.

```bash
attx profile infer --input "scene.scn" --output ./scene.toml --src ja --name scene
attx profile test --profile ./scene.toml --input "scene.scn" --roundtrip
attx run --input "scene.scn" --profile ./scene.toml --src ja --dst zh
attx profile save --profile ./scene.toml
```

See [format limits](docs/en/formats.md) and [profile authoring](docs/en/profiles.md).

## Quality checks and automatic repair

Before accepting model output, attx checks IDs, line structure and exact protected-token multiplicity. Truncated responses are rejected. Missing or invalid units retry in narrower batches without redoing accepted siblings.

Chinese output receives conservative mechanical normalization: isolated `っ`/`ッ` are removed, `っすよ`/`っす` become `哦`, and `ー` becomes `～` when the surrounding text is convincingly Chinese. Japanese sentences, quoted glyph examples and protected literals are not mechanically erased. `・` remains intact.

RPG Maker message fitting keeps an independent `【speaker】` in slot zero. Dialogue controls remain atomic. The adapter never inserts or removes event commands. If a two-slot message contains a name and an overlong sentence, the body can exceed the 44-cell width estimate; attx reports overflow rather than merging the name into dialogue.

```bash
attx review --workspace ".attx-novel"
attx repair --workspace ".attx-novel"
attx writeback --workspace ".attx-novel" --dry-run
```

Review reports residual source script, `kana_edge`, `kana_mixed`, `kana_untranslated`, identical copies, protected-token loss, namebox drift and glossary advisories. Samples are capped; repair selection is not capped by report samples. Successful writeback persists normalized and reflowed lines into the cache, so later exports describe the rendered lines.

No model can guarantee a perfect translation of every input in one pass. Checks catch specific structural and script problems; they cannot prove meaning, voice, complete extraction or actual game rendering. [Quality and repair](docs/en/quality.md) describes those limits.

## Configuration and cost

Configuration search order is `--config`, then `$ATTX_HOME/setting.toml` if it exists, then the current directory's `setting.toml`. `--client <name>` selects another configured client.

```toml
[translation]
worker_count = 8
rpm = 60
retry_count = 3
retry_delay = 2
batch_chars = 2500
max_context_items = 6
repair_rounds = 2
context_chars = 1200

[glossary]
enabled = false
min_occurrences = 10
max_terms = 200
inject_limit = 30

[learn]
auto_summarize = true
llm_review = false
```

Each selected unit enters at most `1 + retry_count` requests per translation pass. `translate` and `run` allow the initial pass plus `repair_rounds` additional passes for unresolved selected units. Set `repair_rounds = 0` to disable extra review passes; request-level retries remain bounded by `retry_count`. `context_chars` is the total preceding/following context budget per request, not the model's context window.

Glossary generation is opt-in and costs additional model calls. Existing active terms are injected whenever applicable. Learning summaries are local unless `llm_review` is enabled. Learned skip rules still require approval by index before they may remove extracted text; agents must not bulk-approve them.

The [configuration reference](docs/en/configuration.md) covers every setting, optional model parameters, request `extra`, precedence, tuning and environment variables. [Workflows](docs/en/usage.md) covers glossary, preservation and learning commands.

## Safe writeback and honest status

A normal translation command authorizes its ordinary output path; attx does not prompt again for writeback. Safety comes from checks rather than a conversation loop:

- Invalid or pending units block output by default. `--allow-partial` explicitly writes valid units and keeps unresolved originals.
- First existing destinations are backed up once as `{path}.attxbak`. Backup failure stops replacement.
- All output files are staged before replacement. Replacement is atomic per file, not one transaction across a directory.
- Workspace mutations use an OS lock. A workspace is bound to its input, engine, profile snapshot and language pair.
- Changed source units or anchors require extraction again. Existing source snapshots and backups keep RPG Maker re-extraction stable.
- Dry runs never persist normalized translations or replace output files.

| Exit | Meaning |
|---|---|
| `0` | Command completed, or a dry-run plan was returned |
| `1` | Execution, configuration, source integrity or HTTP failure |
| `2` | Translation remains incomplete, writeback was blocked, or explicit partial output needs attention |

For exit 2, JSON remains on stdout with `status`, counts and review details. `doctor` and advisory `review` have their own reporting semantics. Never infer finished translation from process exit alone.

## Use with an agent

The operational contract is [`skills/attx/SKILL.md`](skills/attx/SKILL.md), with guided setup and CLI/recovery references. Use the [explained setup wizard](docs/en/agent-translation.md) for first-time configuration; working settings skip repeat questions, and normal writeback needs no extra approval.

Example request:

```text
Use <attx-dir>/skills/attx/SKILL.md and the installed attx CLI.
Translate <input path> from Japanese to Simplified Chinese.
Run the normal pipeline, repair detected failures within configured limits,
and write the result with backups. Report output paths and unresolved issues.
Do not print credentials or modify inputs/cache by hand.
```

For Claude Code, a local Skill installation can use:

```bash
mkdir -p ~/.claude/skills
cp -a skills/attx ~/.claude/skills/
```

Other agents can read the repository Skill directly. No MCP server or bundled autonomous agent runtime is required. Separate permission remains appropriate for unrelated deletions, workspace resets or expanding paid scope beyond the requested task.

## Documentation and development

The manual is available in [English](docs/en/index.md), [中文](docs/zh/index.md) and [日本語](docs/ja/index.md). Start with [quick start](docs/en/quickstart.md), then use the [CLI reference](docs/en/cli.md), [workspace guide](docs/en/workspace.md), [architecture](docs/en/architecture.md) and [development guide](docs/en/development.md).

```bash
cargo test -- --test-threads=1
cargo clippy --all-targets
cargo build --release
pip install -r requirements-docs.txt
mkdocs build --strict
```

All Rust tests are inline. Adapters implement pure extraction/writeback; the pipeline owns network calls, caching and artifact commits. See the development guide before adding an adapter, profile rule, configuration field or CLI command.

This release independently implements ideas studied in [LinguaGacha](https://github.com/neavo/LinguaGacha): targeted retry, shared agent/batch translation, bounded context, strict structure and protected text. No upstream code was copied. Its commercial-use notice is not an attx license grant. Format-adapter design also draws on [AiNiee](https://github.com/NEKOparapa/AiNiee).

Changes: [CHANGELOG.md](CHANGELOG.md). attx is licensed under [MIT](LICENSE).
