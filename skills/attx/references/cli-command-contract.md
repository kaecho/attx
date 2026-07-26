# attx CLI 命令契约

Agent 只依赖本文件 + 命令 stdout JSON。版本以当前二进制 `--help` 为准。

## 全局

```bash
attx [--config <setting.toml>] [--client <名>] <子命令> ...
```

- 配置缺省：`./setting.toml` → `$ATTX_HOME/setting.toml`
- `--client`：选用 setting.toml 中指定名字的 LLM client（缺省 `default_client`）
- 成功：进程码 0，stdout 为 JSON 对象（部分命令 pretty）
- 失败：进程码非 0，stderr 含 `error: ...`

## doctor

```bash
attx doctor [--ping] [--json]
```

用途：检查配置；`--ping` 发极短请求；`--json` 输出机器可读结构：
`{"llm":{"configured",...},"ping","adapters":[...],"saved_profiles":[...],"status"}`
（不含 api_key）。失败常见：无 clients、HTTP 401、超时。

## formats

```bash
attx formats
```

stdout：`{"formats":[{"id","label","extensions":[],"input":"file|directory","profile"?}]}`  
含已保存的自定义 Profile（`id` 为 `custom:<名>`，附 `profile` 路径）。  
用途：Agent 判断某输入能否翻译、向用户展示能力清单。

## detect

```bash
attx detect --input <文件或目录>     # --game 为兼容别名
```

stdout 示例：

```json
{"engine":"epub","content_root":"/abs/path/book.epub","label":"EPUB e-book","profile":null}
```

字段：`engine`、`content_root`（文件输入时即文件路径）、`label`、`profile`
（命中已保存自定义 Profile 时为其路径）。  
`.json` 输入按内容嗅探（paratranz → vnt → mtool → i18next）；歧义时加 `--engine`。  
失败（无适配器）→ 走 `custom-format-discovery.md`。

## analyze

```bash
attx analyze --input <文件或目录> [--src ja|en]
```

未知格式侦察报告（JSON）：`builtin_detect` / `saved_profile_detect`、
文件时 `details.encoding`（含 Shift_JIS/GBK 检测）、`total_lines`、
`source_language_lines`、`json`（top_keys / 数组首元素）、`sample_head[]`；
目录时 `extensions` 直方图 + `peek`（抽样一个文本文件的完整分析）。
二进制时 `binary:true` + `container` 提示。

## profile（自定义格式）

```bash
attx profile new  --output ./fmt.toml [--name <名>]      # 带注释模板
attx profile test --profile <路径或已存名> --input <输入> [--src ja] [--limit 10] [--roundtrip]
attx profile save --profile ./fmt.toml [--force]         # 记住格式（写入用户 profile 目录）
attx profile list                                        # {"profiles":[...],"dirs":[...]}
```

`test` stdout：`units`、`sample[{location,role,text}]`、`detects`、
`--roundtrip` 时附 `roundtrip{ok,output_files,outputs[]}`（内存中执行，不写盘）。  
保存目录：`$ATTX_HOME/profiles/` 或 `~/.config/attx/profiles/`。
规则语法详见 `custom-format-discovery.md` 与仓库 `profiles/examples/`。

## init

```bash
attx init --input <文件或目录> --src ja|en --dst zh [--engine <id>] [--profile <路径或名>] [--workspace <目录>]
```

- 默认工作区：目录输入 `<content_root>/.attx`；文件输入 `<父目录>/.attx-<文件名去扩展名>`
- `--profile`：自定义 Profile（.toml 路径或已保存名）；会拷贝为 `<工作区>/profile.toml`，
  engine 记为 `custom:<名>`；`--engine custom:<名>` 等价于用已保存 Profile
- `--dst`：目标语言（`zh`/`zh-tw`/`en`/`ko` 等，写入译文 prompt 与输出文件名）
- stdout：`{"workspace":"...","status":"ok"}`
- 副作用：创建 `attx.db`、`workspace.json`

## extract

```bash
attx extract --workspace <工作区>
```

stdout：`{"extracted":N,"status":"ok"}`  
副作用：重建 `units` 表；不匹配的旧译文会清理。

## status

```bash
attx status --workspace <工作区>
```

```json
{
  "engine": "rmmz",
  "game_path": "...",
  "source_lang": "ja",
  "target_lang": "zh",
  "total": 12341,
  "translated": 20,
  "pending": 12321,
  "passthrough": 0,
  "domains": {"dialogue": {"total": 9000, "translated": 15}}
}
```

`passthrough`：模型拒答/失败后以原文占位的条数（算已译）；
收尾时 >0 应报告用户并可 `translate --retry-passthrough` 重试。

## translate

```bash
attx translate --workspace <工作区> [--limit N] [--dry-run] [--retry-passthrough]
```

- 只处理 pending（无有效译文或源文 hash 变化）
- `--limit`：最多翻译 N 条（试译必用）
- `--dry-run`：不请求模型
- `--retry-passthrough`：先清掉 passthrough 占位译文使其重新 pending
- stdout：`pending_before` / `translated` / `pending_after` / `passthrough` / `dry_run` / `skipped_note`
- stderr：`batch i/n (...)` 进度
- 并发请求受 `[translation].rpm` 全局限速（0 = 不限速）

## writeback

```bash
attx writeback --workspace <工作区> [--dry-run]
```

- 输出目标由适配器决定：
  - `rmmz`：**原地**写回游戏 `data/*`、`js/plugins.js`（已有文件先备份 `*.attxbak`）
  - 文档/字幕/JSON 类：写**翻译副本** `<名>.<目标语言>.<扩展名>`（原文件不动）
  - `jsonl`：`translated.jsonl`
- stdout：`files`、`units_applied`、`dry_run`、`paths[]`（绝对路径）

## run

```bash
attx run --input <文件或目录> --src ja --dst zh \
  [--workspace] [--engine] [--limit] [--no-translate] [--no-writeback]
```

顺序：init → extract → [translate] → [writeback]。  
**rmmz 含写回时等同需要用户写回许可；文档类无需。**

## translate-jsonl

```bash
attx translate-jsonl --input in.jsonl --output out.jsonl --src ja --dst zh [--limit N]
```

无需工作区。输入行至少：`id`、`text`；可选 `context`、`role`、`item_type`。  
输出附加 `translation`、`translation_lines`。

## export-jsonl / import-jsonl

```bash
attx export-jsonl --workspace <工作区> --output out.jsonl --filter pending|all|translated|passthrough
attx import-jsonl --workspace <工作区> --input in.jsonl
```

import 按 `id` == unit.`location` 匹配；需要非空 `translation_lines` 或 `translation`。

## setting.toml（LLM）

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"
base_url = "https://api.example.com/v1"
api_key = "..."
model = "..."
timeout = 600

[translation]
worker_count = 8
rpm = 60          # 全局请求限速（次/分钟），0 = 不限
retry_count = 3
retry_delay = 2
batch_chars = 2500
max_context_items = 6
```

Agent 可读 `setting.example.toml`；**禁止**在对话中回显 `api_key`。
