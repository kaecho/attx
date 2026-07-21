# attx CLI 命令契约

Agent 只依赖本文件 + 命令 stdout JSON。版本以当前二进制 `--help` 为准。

## 全局

```bash
attx [--config <setting.toml>] <子命令> ...
```

- 配置缺省：`./setting.toml` → `$ATTX_HOME/setting.toml`
- 成功：进程码 0，stdout 为 JSON 对象（部分命令 pretty）
- 失败：进程码非 0，stderr 含 `error: ...`

## doctor

```bash
attx doctor
attx doctor --ping
```

用途：检查配置；`--ping` 发极短请求。  
失败常见：无 clients、HTTP 401、超时。  
`adapters:` 行列出全部格式 id。

## formats

```bash
attx formats
```

stdout：`{"formats":[{"id","label","extensions":[],"input":"file|directory"}]}`  
用途：Agent 判断某输入能否翻译、向用户展示能力清单。

## detect

```bash
attx detect --input <文件或目录>     # --game 为兼容别名
```

stdout 示例：

```json
{"engine":"epub","content_root":"/abs/path/book.epub","label":"EPUB e-book"}
```

字段：`engine`、`content_root`（文件输入时即文件路径）、`label`。  
`.json` 输入按内容嗅探（paratranz → vnt → mtool → i18next）；歧义时加 `--engine`。

## init

```bash
attx init --input <文件或目录> --src ja|en --dst zh [--engine <id>] [--workspace <目录>]
```

- 默认工作区：目录输入 `<content_root>/.attx`；文件输入 `<父目录>/.attx-<文件名去扩展名>`
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
  "pending": 12321
}
```

## translate

```bash
attx translate --workspace <工作区> [--limit N] [--dry-run]
```

- 只处理 pending（无有效译文或源文 hash 变化）
- `--limit`：最多翻译 N 条（试译必用）
- `--dry-run`：不请求模型
- stdout：`pending_before` / `translated` / `pending_after` / `dry_run`
- stderr：`batch i/n (...)` 进度

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
attx export-jsonl --workspace <工作区> --output out.jsonl --filter pending|all|translated
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
rpm = 60
retry_count = 3
retry_delay = 2
batch_chars = 2500
max_context_items = 6
```

Agent 可读 `setting.example.toml`；**禁止**在对话中回显 `api_key`。
