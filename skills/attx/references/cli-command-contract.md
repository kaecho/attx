# attx CLI 命令契约

Agent 只依赖本文件 + 命令 stdout JSON。版本以当前二进制 `--help` 为准。

## 全局

```bash
attx [--config <setting.toml>] [--client <名>] <子命令> ...
```

- 配置缺省：`--config` → `$ATTX_HOME/setting.toml` → `./setting.toml`
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

## review

```bash
attx review --workspace <工作区>
```

机械审校，不调模型。stdout JSON：

```json
{
  "total": 100,
  "translated": 90,
  "pending": 8,
  "passthrough": 2,
  "glossary": {"active_terms": 12, "terms_seen": 12, "terms_fully_applied": 10, "violations": []},
  "residual_source": {"count": 1, "sample": [{"location": "...", "unit_id": "...", "detail": "..."}]},
  "identical": {"count": 0, "sample": []},
  "control_loss": {"count": 0, "sample": []},
  "namebox_mismatch": {"count": 0, "sample": []}
}
```

`sample` 每类最多 40 条。全量清单走 `export-jsonl`。`attx run` 在 translate 之后也会附带 `review`。
有命中 → 向用户报告，用 export/import 修，不要假装完成。

## preserve

```bash
attx preserve list   --workspace <工作区>
attx preserve add    --workspace <工作区> --pattern '<regex>' [--info <说明>]
attx preserve remove --workspace <工作区> --pattern '<regex>'
```

命中的片段在送模型前变成 `[CTRL_n]`，写回前还原。内置：RMMZ 控制符、`{ident}`、`%s`/`%d`；`renpy` 引擎额外保护 `[ident]`。工作区规则写在 `preserve.toml`。空匹配 / 非法正则拒绝。

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
attx writeback --workspace <工作区> [--dry-run] [--no-learn]
```

- 输出目标由适配器决定：
  - `rmmz`：**原地**写回游戏 `data/*`、`js/plugins.js`（已有文件先备份 `*.attxbak`）
  - 文档/字幕/JSON 类：写**翻译副本** `<名>.<目标语言>.<扩展名>`（原文件不动）
  - `jsonl`：`translated.jsonl`
- stdout：`files`、`units_applied`、`dry_run`、`paths[]`（绝对路径）
- 非 dry-run 且 `[learn].auto_summarize` 为真时，额外返回 `learned`（见 learn 一节）；
  `--no-learn` 关闭本轮沉淀。经验总结失败只打 stderr，**不会**让 writeback 失败。

## run

```bash
attx run --input <文件或目录> --src ja --dst zh \
  [--workspace] [--engine] [--limit] [--no-translate] [--no-writeback] \
  [--glossary | --no-glossary]
```

顺序：init → extract → [glossary build] → [translate] → [writeback]。  
术语表仅在 `[glossary].enabled = true` 或显式 `--glossary` 时构建；`--no-glossary`
可在配置已开启时单次跳过。构建失败只记入 `glossary.error`，不中断整轮。  
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

## glossary（术语表，默认关闭）

```bash
attx glossary build  --workspace <工作区> [--min-occurrences N] [--dry-run]
attx glossary list   --workspace <工作区> [--all]
attx glossary add    --workspace <工作区> --src <原文> --dst <译名> [--info <消歧描述>] [--case-sensitive]
attx glossary remove --workspace <工作区> --src <原文>
attx glossary import --workspace <工作区> --file <json>
attx glossary export --workspace <工作区> --file <json>
attx glossary check  --workspace <工作区>
```

提取全程由 LLM 负责（LinguaGacha 策略）：原文分批交给模型直接抽
`{src,dst,info}`（专有名词与作品特有概念：人名/地名/家族/组织/物品/技能/生物/概念），
之后机械把关两道：

1. **子串闸门**：`src` 必须是源文真实子串（防幻觉）；
2. **`min_occurrences` 门槛**：按原文行级出现次数过滤偶发词
   （`[glossary].min_occurrences`，默认 10；CLI `--min-occurrences` 可覆盖；
   namebox 说话人铭牌不受此限）。

同一 `src` 跨批次投票，译名/类型多数胜出；namebox 铭牌置前排。
费用与文本量（批次数）成正比。

`build` 报告字段：

| 字段 | 含义 |
|------|------|
| `candidates` | 聚合后的唯一 src 数（过子串闸门） |
| `above_threshold` | 出现次数 ≥ `min_occurrences` 的数量 |
| `truncated` | 被 `max_terms` 砍掉的数量（>0 时说明覆盖不全，需向用户报告） |
| `asked` | 送往模型的原文批次数 |
| `added` / `rejected` | 写入 / 过滤掉的数量 |
| `total_active` | 当前生效术语总数 |
| `min_occurrences` | 本次生效的出现次数门槛 |
| `sample` | 样本（已写入条目） |

**必须先 `--dry-run`**：不调用模型，看 `asked`（批次数）向用户报告规模与预计费用。
取得同意后再真建。`build` 显式调用时不受 `[glossary].enabled` 限制。

`check` 输出 `violations[]`（`src`/`dst`/`occurrences`/`applied`），按未生效次数降序。
子串比对，屈折语可能误报，是审阅辅助不是硬门禁。

`import` 接受两种 JSON：`[{"src","dst","info"}]` 或 `{"src": "dst"}`。

## learn（经验层）

```bash
attx learn summarize --workspace <工作区> [--llm]   # scan 是其别名
attx learn note --text "…" [--name <短名>] [--topic prompt] --workspace <工作区>
attx learn note --text "…" [--name <短名>] --format <id>   # 写入全局知识库
attx learn pending
attx learn review --approve 1,3 | --reject 2 | --approve-all
attx learn list [--format <id>] [--workspace <工作区>]
attx learn defaults --format <id>       # 打印内置基线 TOML
attx learn forget --field <字段名> [--format <id>]
attx learn forget --name <短名> [--workspace <工作区> | --format <id>]
attx extract --no-knowledge             # 逃生舱
attx writeback --no-learn               # 本轮不沉淀提取经验
```

`writeback` 成功后**自动**执行 summarize（零 API 成本），结果在
`writeback.learned`：`entries_written` / `pending` / `notes` / `file`。
这只沉淀**提取**判断（哪些字段不该译）和客观信号（控制码丢失）。

`pending > 0` 表示存在会**删除文本**的 `skip` 条目待批准。
**agent 不得自行 `--approve-all`**——报告条数与证据，交用户裁决。

翻译文风用 `learn note`，不要用 `summarize --llm`：

- `--text`：一条具体指令。`--topic` 默认 `prompt`，会注入下一轮 `translate` 的系统提示词。
- `--name`：upsert 键（source = `learn:agent:<name>`）。同名覆盖，异名并存。自动 summarize 不会改这些条目。
- `--workspace`：写入 `<工作区>/experience.toml`（本作品）。`--format` 且无 workspace：写入 `$ATTX_HOME/knowledge/<format>.toml`。
- 必须给 `--workspace` 或 `--format` 之一。两者都给时写入工作区，format 用参数值。
- stdout：`format` / `topic` / `name` / `source` / `text` / `file` / `layer`（`workspace`|`global`）/ `reaches_prompt`。
- `topic` 不是 `prompt` 时 stderr 警告：已存储但不会进翻译。
- 专有名词走 `glossary add`，不走 note。

`learn list --workspace` 只列出该工作区的 `experience.toml`（带 `"layer": "workspace"`）。不加则列出全局知识文件。

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
# temperature = 0.3            # omit: translate 0.3, glossary/learn JSON 0.0
# reasoning_effort = "medium"  # omit: not sent
# max_tokens = 8192            # omit: not sent
# stream = true                # omit: false; SSE delta.content
# extra = { top_p = 0.9 }      # merged last; cannot replace messages

[translation]
worker_count = 8
rpm = 60          # 全局请求限速（次/分钟），0 = 不限
retry_count = 3
retry_delay = 2
batch_chars = 2500
max_context_items = 6

[glossary]
enabled = false        # 默认关闭：构建术语表有额外 LLM 费用
min_occurrences = 10   # LLM 提取的术语须在原文出现 ≥ 此次数才入表
max_terms = 200        # 保留术语上限
inject_limit = 30      # 单批注入提示词的术语上限

[learn]
auto_summarize = true  # writeback 后沉淀经验（免费）
llm_review = false     # 额外让模型复核提案（有费用）
```

Agent 可读 `setting.example.toml`；**禁止**在对话中回显 `api_key`。
