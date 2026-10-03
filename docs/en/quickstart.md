# Quick start

Want the agent to handle installation, configuration, and commands? Use [Translate with an agent](agent-translation.md). Copy its prompt and answer the questions; you do not need to edit TOML or install the skill first. The dialogue belongs to your coding agent, not a built-in attx setup command.

The optional manual path below translates a small Japanese text file into Simplified Chinese. It uses explicit configuration and workspace paths so the files are easy to find. Install attx first using [installation](install.md).

## Configure locally (manual path)

Copy the shipped example and edit it in a local editor:

```bash
cp "./setting.example.toml" "./setting.toml"
```

A minimal configuration is:

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
base_url = "https://api.example.com/v1"
api_key = "YOUR_LOCAL_API_KEY"
model = "your-provider-model"
```

Replace all three placeholder values. `base_url` is the API prefix, not the full `/chat/completions` URL. `provider_type` is optional and does not select a native vendor protocol. All requests use OpenAI-compatible Chat Completions. Remaining sections use the [documented defaults](configuration.md).

```bash
attx --config "./setting.toml" doctor --json
attx --config "./setting.toml" doctor --ping --json
```

Read `llm.configured` and the ping result. Stop on an invalid endpoint, model, or authentication error instead of repeatedly pinging the same credentials.

## Create and translate a first input

On Linux or macOS:

```bash
mkdir -p "./translation demo"
printf '%s\n' 'こんにちは。今日はいい天気ですね。' 'また明日会いましょう。' > "./translation demo/sample.txt"
attx detect --input "./translation demo/sample.txt"
attx --config "./setting.toml" run \
  --input "./translation demo/sample.txt" --src ja --dst zh \
  --workspace "./translation demo/workspace"
```

The source remains `translation demo/sample.txt`. The `txt` adapter writes `translation demo/sample.zh.txt`. The cache and readable metadata are in `translation demo/workspace/attx.db` and `workspace.json`.

PowerShell equivalent, with UTF-8 input:

```powershell
Copy-Item ".\setting.example.toml" ".\setting.toml"
notepad ".\setting.toml"
.\attx.exe --config ".\setting.toml" doctor --ping --json
New-Item -ItemType Directory -Force ".\translation demo" | Out-Null
$text = "こんにちは。今日はいい天気ですね。`nまた明日会いましょう。`n"
[System.IO.File]::WriteAllText((Join-Path (Get-Location) "translation demo\sample.txt"), $text, [System.Text.UTF8Encoding]::new($false))
.\attx.exe detect --input ".\translation demo\sample.txt"
.\attx.exe --config ".\setting.toml" run `
  --input ".\translation demo\sample.txt" --src ja --dst zh `
  --workspace ".\translation demo\workspace"
$LASTEXITCODE
```

If attx is on `PATH`, replace `.\attx.exe` with `attx`. Paths containing spaces need quotes. PowerShell uses a backtick for line continuation, not Bash's backslash.

## Read the result before calling it complete

`run` prints one final JSON object containing the workspace, extraction, translation, review, writeback if performed, and `status`. Read `writeback.paths` for actual destinations.

- Exit 0 means the requested operation completed. An extraction-only run is not a translated file.
- Exit 1 means a fatal error. Successful earlier batches may still be cached.
- Exit 2 means the structured result is incomplete or writeback is blocked. Parse stdout even when the process is nonzero.

If unresolved units remain, default writeback produces no translated artifacts. It does not silently write source placeholders and call the work successful.

```bash
attx status --workspace "./translation demo/workspace"
attx review --workspace "./translation demo/workspace"
attx --config "./setting.toml" repair --workspace "./translation demo/workspace"
attx --config "./setting.toml" writeback --workspace "./translation demo/workspace"
```

Do not loop repair indefinitely. Each command has a bounded request budget; repeated invocations start another budget. Use [manual JSONL correction](workspace.md) when problems persist.

## Optional previews and trials

To inspect without translating:

```bash
attx run --input "./novel.epub" --src ja --dst zh --no-translate --no-infer
```

For a user-chosen 20-unit trial without artifact writeback:

```bash
attx --config "./setting.toml" run --input "./novel.epub" --src ja --dst zh \
  --limit 20 --no-writeback
```

A trial may return exit 2 because the rest of the work is still pending. It is not a full translation failure and is not full completion either. `run` has no `--dry-run`; stage commands do:

```bash
attx --config "./setting.toml" translate --workspace "./translation demo/workspace" --dry-run
attx writeback --workspace "./translation demo/workspace" --dry-run
```

Dry-run reports plans and does not call the translation model or persist normalized cache text. A dry-run exit 0 is never evidence that outputs were written.

## Use a real project

```bash
attx --config "./setting.toml" run --input "./novel.epub" --src ja --dst zh
attx --config "./setting.toml" run --input "./My Game" --src ja --dst zh
```

The EPUB normally produces `novel.zh.epub`. The RPG Maker example writes live game data and keeps `.attxbak` backups; read [RPG Maker](rmmz.md) first. Unknown text may use local Auto detection or a paid bounded profile inference. Add `--no-infer` if you do not want format-inference requests.

Next: [workflows](usage.md), [formats](formats.md), and the [complete CLI reference](cli.md).