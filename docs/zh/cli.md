# CLI

`attx [--config <path>] [--client <name>] <command> [options]`

成功 → stdout 输出 JSON，退出码 0。失败 → stderr 输出 `error: …`，非零退出码。

stdout JSON 的确切结构固定在 `skills/attx/references/cli-command-contract.md` —— agent 应依赖该文件以及当前二进制的 `--help`。

## 全局选项

| 选项 | 含义 |
|--------|---------|
| `--config <path>` | `setting.toml` 的路径（默认：`$ATTX_HOME/setting.toml` → `./setting.toml`） |
| `--client <name>` | 改用这个 `[[llm.clients]]` 条目，而不是 `[llm].default_client` |

`--input` 也接受别名 `--game`。

## 命令参考

### `doctor [--ping] [--json]`

检查配置与可选的 LLM 连通性。普通输出人类可读；`--json` 输出 `{"llm":{configured,error},"ping","adapters":[],"saved_profiles":[],"status"}`（绝不包含 API Key）。

### `formats`

以 JSON 列出支持的适配器与已保存的 Profile：`{"formats":[{"id","label","extensions":[],"input":"file|directory"}]}`。已保存的自定义 Profile 以 `id = "custom:<name>"` 出现。

### `detect --input <path>`

探测格式。JSON 输出：`{"engine","content_root","label","profile"}`。先内置适配器，再已保存的自定义 Profile。

### `analyze --input <path> [--src ja|en]`

面向未知输入的侦察报告：`builtin_detect`、`saved_profile_detect`；文件输入给出 `details`（大小、`binary`、`encoding`（含 Shift-JIS/GBK 检测）、行数、JSON 形态、`sample_head`），目录输入给出扩展名直方图 + 抽样查看的样本文件。最后给出 `next_steps` 建议。

### `profile`

| 子命令 | 作用 |
|------------|------|
| `new --output <path> [--name <name>]` | 写出带注释的规则模板 |
| `test --profile <path|name> --input <path> [--src] [--limit 10] [--roundtrip]` | 试提取；报告匹配的单元（`--roundtrip` 时附带内存中写回） |
| `save --profile <path> [--force]` | 把 Profile 记住到用户 Profile 目录 |
| `list` | 已保存的 Profile（JSON） |

### `init --input <path> --src ja|en --dst zh [--engine <id>] [--profile <path|name>] [--workspace <dir>]`

注册 / 打开工作区（创建 `attx.db`、`workspace.json`）。`--engine` 强制使用内置适配器；`--profile` 指定自定义 Profile（复制到 `<workspace>/profile.toml`，engine 记为 `custom:<name>`）。默认工作区：`<dir>/.attx` 或 `<parent>/.attx-<stem>`。

### `extract --workspace <dir> [--no-knowledge]`

适配器 → 文本单元入库。JSON 输出：`{"extracted","skipped_by_knowledge","rules_applied","status"}`。`--no-knowledge` 忽略所有学到的规则（即没有经验层时的行为）。

### `translate --workspace <dir> [--limit N] [--dry-run] [--retry-passthrough]`

对待译单元调用 LLM，增量保存。JSON 输出：`{"pending_before","translated","pending_after","passthrough","dry_run","skipped_note"}`。stderr 显示 `batch i/n` 进度。`--dry-run` 只打印批次计划，不调用模型；`--retry-passthrough` 先把 passthrough 单元重新入队。

### `writeback --workspace <dir> [--dry-run] [--no-learn]`

渲染翻译输出。JSON 输出：`{"files","units_applied","dry_run","paths":[]}` —— 自动的写回后经验总结运行时还会给出 `learned`。`--dry-run` 只做计划；`--no-learn` 跳过本次运行的经验总结。

### `run --input <path> [--engine] [--profile] [--src] [--dst] [--workspace] [--limit] [--no-translate] [--no-writeback] [--glossary] [--no-glossary]`

init → extract →（启用或强制时构建术语表）→ translate → writeback，一份 JSON 报告。`--glossary` 即使 `[glossary].enabled` 为 false 也会构建术语表；`--no-glossary` 禁止本次运行构建。术语表构建失败不致命（在 `glossary.error` 下报告）。

### `status --workspace <dir>`

`{"engine","game_path","source_lang","target_lang","total","translated","pending","passthrough","domains":{…}}`。`passthrough > 0` → 考虑 `translate --retry-passthrough`。

### `review --workspace <dir>`

机械审校，不调模型。JSON：`total` / `translated` / `pending` / `passthrough`，以及 `glossary`（同 `glossary check`）和分桶 `residual_source`、`identical`、`control_loss`、`namebox_mismatch`（`count` + 最多 40 条 `sample`）。`attx run` 在 translate 之后附带这份报告。

### `preserve`

| 子命令 | 作用 |
|------------|------|
| `list --workspace <dir>` | 内置 + 工作区正则（JSON） |
| `add --workspace --pattern <re> [--info]` | 追加工作区规则；命中变成 `[CTRL_n]` |
| `remove --workspace --pattern <re>` | 按精确 pattern 删除工作区规则 |

内置始终生效：RMMZ 控制符、`{ident}`、`%s`/`%d`。引擎 `renpy` 额外保护 `[ident]`。工作区文件：`preserve.toml`。

### JSONL 交换

| 命令 | 作用 |
|---------|------|
| `translate-jsonl --input --output [--src] [--dst] [--limit]` | 无需工作区：输入 `{id,text,context?,role?,item_type?}`，输出追加 `translation`+`translation_lines` |
| `export-jsonl --workspace --output [--filter pending\|all\|translated\|passthrough]` | 工作区 → JSONL |
| `import-jsonl --workspace --input` | JSONL → 工作区（按 `id` 匹配） |

### `learn`

| 子命令 | 作用 |
|------------|------|
| `summarize --workspace <dir> [--llm]` | 把运行证据转化为 skip 字段条目（`scan` 是别名）；`--llm` 复核这些提案（有费用） |
| `note --text "…" [--name <id>] [--workspace <dir> \| --format <id>]` | 写一条文风 note。默认 `topic=prompt`，下一轮 translate 会注入系统提示词 |
| `pending` | 待批准的条目，带证据（JSON） |
| `review --approve 1,3 [--reject 2] [--approve-all]` | 按 1 起始的索引批准/拒绝 |
| `list [--format <id>] [--workspace <dir>]` | 生效中的条目（JSON）。`--workspace` 列出本作品的 `experience.toml` |
| `defaults --format <id>` | 打印某格式的内置基线（TOML） |
| `forget --field <name> [--format <id>]` | 按字段名删除 skip/extract 条目 |
| `forget --name <id> [--workspace <dir>]` | 删除 `learn note` 写的 note |

### `glossary`

| 子命令 | 作用 |
|------------|------|
| `build --workspace <dir> [--method llm\|stats] [--min-occurrences N] [--dry-run]` | 把专有名词提取进 `glossary.toml`；务必先试运行 |
| `list [--all]` | 术语（JSON）（`--all` 包含模型否决的） |
| `add --src <term> --dst <translation> [--info <desc>]` | 添加/覆盖一个术语 |
| `remove --src <term>` | 删除一个术语 |
| `import --file <json>` / `export --file <json>` | `[{src,dst,info}]` 或 `{src: dst}` |
| `check` | 译文实际未使用的术语（违规项，按次数） |
