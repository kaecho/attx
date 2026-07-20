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

## detect

```bash
attx detect --game <游戏目录>
```

stdout 示例：

```json
{"engine":"rmmz","content_root":"/abs/path","label":"RPG Maker MV/MZ"}
```

字段：`engine`、`content_root`、`label`。

## init

```bash
attx init --game <游戏目录> --src ja|en --dst zh [--engine rmmz|jsonl] [--workspace <目录>]
```

- 默认工作区：`<content_root>/.attx`
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

- 将已译单元写回游戏 `data/*`（rmmz）或 `translated.jsonl`（jsonl）
- 真写回前对已有文件生成 `*.attxbak`（若不存在）
- stdout：`files`、`units_applied`、`dry_run`、`paths[]`

## run

```bash
attx run --game <游戏目录> --src ja --dst zh \
  [--workspace] [--engine] [--limit] [--no-translate] [--no-writeback]
```

顺序：init → extract → [translate] → [writeback]。  
**含写回时等同需要用户写回许可。**

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
