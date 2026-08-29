# attx

**English** | [中文](README.zh-CN.md) | [Docs](https://kaecho.github.io/attx/)

**Agent Translation Toolkit eXtensible** —— 一个纯 Rust、单二进制、格式无关的 AI 翻译框架，面向 agent 与人类用户。

```
extract (format adapter) → translate (LLM core) → writeback (format adapter)
```

使用任意 OpenAI 兼容的 LLM 翻译电子书、文档、字幕、本地化文件与游戏。进度缓存在 SQLite 工作区中，中断的运行可免费续跑。格式支持以 [AiNiee](https://github.com/NEKOparapa/AiNiee) 的读写插件集为蓝本，用 Rust 适配器重新实现。

- **19 个内置适配器** —— EPUB、HTML、DOCX、XLSX、TXT/MD、SRT/VTT/ASS/LRC、CSV、PO、Ren'Py、RPG Maker MV/MZ、MTool/Paratranz/VNTextPatch/i18next JSON，外加一个通用 JSONL 交换格式。
- **自定义格式 Profile** —— 用一个小 TOML 文件（`line_regex` / `json_keys` / `json_paths` 规则）教 attx 认识任何未知的文本/JSON 格式。
- **设计上可续跑** —— 每次运行都在 `attx.db` 中打点存档；随时可停，随时可续。失败的单元会变成可见的 *passthrough* 占位，而不是终止整个运行。
- **自我改进** —— 成功的运行会留下提取经验（`skip`/`extract` 字段判断），由你审阅，绝不会悄悄应用而删除文本。
- **术语表** —— 为整部作品的每个专有名词约定一个译名，按批次注入。
- **回检** —— 翻译后免费机械扫描：残留假名、原文照抄、丢失保护码、姓名栏与对白不一致。

---

## Agent 快速上手（推荐）

attx 的设计目标是让编程 agent 能够**阅读 Skill、问你几个问题、写好 `setting.toml` 并运行流水线** —— 你不需要先手工编辑配置。

### 1. 安装二进制

- **发行版：** [Releases](https://github.com/kaecho/attx/releases)（tag `v*`）
- **从源码构建：**

```bash
git clone https://github.com/kaecho/attx.git
cd attx
cargo build --release
./target/release/attx --help
# optional:
cargo install --path .
```

### 2. 安装 Skill（让 agent 了解协议）

```text
skills/attx/SKILL.md           # stages, hard stops, Q&A config wizard
skills/attx/references/        # CLI contract, agent usage, custom-format discovery, recovery, JSONL, feedback
```

**Claude Code：**

```bash
# personal, all sessions:
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
# or project-scoped:
mkdir -p .claude/skills && cp -a skills/attx .claude/skills/
```

**其他任何 agent**（Cursor / Codex / OpenCode / ……）：保留检出目录并说：

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
```

**为什么用 Skill 而不是 MCP 服务器？** attx 是一个本地 CLI，stdout 输出 JSON —— 这已经是编程 agent 的原生工具面。Skill 是任何 agent 都能遵循的纯 Markdown；MCP 只是把同一个 CLI 包在一个常驻进程后面。

### 3. 一条提示词 —— agent 通过 Q&A 配置，然后翻译

如果 `setting.toml` 缺失或 `attx doctor` 失败，Skill **要求**进行交互式向导。agent 一次问一项：

1. API 端点（OpenAI / DeepSeek / 自定义 OpenAI 兼容 `base_url`）
2. API Key → 只写入 `setting.toml`，**绝不**回显到聊天中
3. 模型名
4. 语向（`src` / `dst`）
5. 可选：并发 / 术语表

然后运行 `attx doctor --ping` 并继续流水线。

复制粘贴：

```text
Use the attx toolkit at <attx-dir>, following skills/attx/SKILL.md.

Help me set up attx if needed (Q&A wizard: endpoint, key, model, languages),
then translate <input path> from Japanese into Simplified Chinese.

Rules:
1. Only operate through the attx CLI; never hand-edit inputs, attx.db, or tool source.
2. If the LLM is not configured, run the Q&A config wizard first; never print my API key.
3. doctor --ping → detect → init → extract → status → translate --limit 20 → full translate.
4. Prefer translated copies for files; ask before any in-place overwrite.
5. Report counts and next step after each stage.
```

更短的形式也可以：

```text
Help me set up attx, then translate ./novel.epub from Japanese to Simplified Chinese.
```

---

## 快速上手（手动）

```bash
cp setting.example.toml setting.toml   # fill base_url / api_key / model
attx doctor --ping                     # verify config + LLM connectivity
attx run --input novel.epub --src ja --dst zh
# → writes novel.zh.epub next to the input; the original is never touched
```

对于大型输入，请使用分步流水线，先用小的 `--limit` 试译（见 [用法](#用法)）。

---

## 配置 LLM

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"          # OpenAI-compatible Chat Completions
base_url = "https://your-provider.example/v1"
api_key = "YOUR_API_KEY"
model = "your-model-name"
timeout = 600                     # seconds
# temperature = 0.3               # 省略则翻译 0.3，glossary/learn JSON 0.0
# reasoning_effort = "medium"     # 省略则不发送
# max_tokens = 8192               # 省略则不发送
# stream = true                   # 省略则 false；按 SSE delta.content 拼接
# extra = { top_p = 0.9 }         # 最后合并进请求体；不能替换 messages

[translation]
worker_count = 8       # parallel HTTP batches
rpm = 60               # global request rate limit per minute (0 = unlimited)
retry_count = 3
retry_delay = 2        # seconds between retries
batch_chars = 2500     # max source chars per batch
max_context_items = 6  # max units per batch

[glossary]
enabled = false        # build during `attx run` (costs extra LLM calls)
method = "llm"         # llm | stats

[learn]
auto_summarize = true  # capture experience after writeback (free)
llm_review = false     # also ask the model to check proposals (costs money)
```

`setting.toml` 已被 gitignore —— 绝不提交 API Key。用 `attx doctor --ping` 验证。

配置查找顺序：`--config` → `$ATTX_HOME/setting.toml` → `./setting.toml`。`--client <name>` 可在单次调用中切换 LLM 客户端。

---

## 用法

### 一键运行

```bash
attx run --input "novel.epub" --src ja --dst zh
# → novel.zh.epub next to the input
```

`run` = `init` → `extract` →（可选术语表）→ `translate` → `writeback`，每个阶段以 JSON 报告。加 `--limit 20` 试译，`--no-writeback` 在写回前检查，`--glossary`/`--no-glossary` 覆盖配置。

### 分步执行（大型输入 —— 先试译 20 个单元）

```bash
attx detect  --input book.epub
attx init    --input book.epub --src ja --dst zh      # workspace: .attx-book/
attx extract --workspace .attx-book
attx status  --workspace .attx-book
attx translate --workspace .attx-book --limit 20      # trial
attx translate --workspace .attx-book                 # full; re-run to resume
attx writeback --workspace .attx-book --dry-run       # preview planned files
attx writeback --workspace .attx-book                 # → book.zh.epub
```

工作区布局：目录输入使用 `<dir>/.attx`；文件输入使用 `<parent>/.attx-<stem>` —— 内含 `attx.db`（单元 + 译文 + 元数据）、`workspace.json`，可选 `glossary.toml`、`experience.toml`、`profile.toml`。

大多数文件格式写出**翻译副本**（`*.<dst>.*`），源文件保持不动。`rmmz` 游戏适配器**原地**写回，带一次性 `*.attxbak` 备份 —— 务必先 `writeback --dry-run`。

真实世界验证：一部 4,171 段的全本轻小说 EPUB（含插图 10.9 MB）一次运行 ja→zh-Hans 全部译完 —— 覆盖率 100%，EPUB 结构与插图完好，目录及 `dc:title`/`dc:language` 已本地化。

### 当模型失败时：passthrough

如果某个单元的翻译反复失败，attx 会把原文存为带标记的 **passthrough** 占位，以便运行完成。`attx status` 报告数量；`attx translate --retry-passthrough` 精确地重新入队这些单元。

### 手动 / 离线审校（JSONL）

```bash
attx export-jsonl --workspace .attx-book --output pending.jsonl --filter pending
# review/edit translation_lines externally, then:
attx import-jsonl --workspace .attx-book --input pending.jsonl
attx writeback    --workspace .attx-book
```

独立使用，无需工作区：

```bash
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh
```

---

## 支持的格式

| id | 输入 | 说明 | 输出 |
|----|------|------|------|
| `epub` | `.epub` | 电子书 / 轻小说：段落级，注音假名（`<rt>`）从源文中剔除，图片与排版保留，`dc:language` 更新 | `<name>.<dst>.epub` |
| `html` | `.html` `.htm` `.xhtml` | 独立 HTML 页面：块级 + `<title>` | 翻译副本 |
| `docx` | `.docx` | Word 文档：段落级，覆盖 `w:t` run | `<name>.<dst>.docx` |
| `xlsx` | `.xlsx` `.xlsm` | Excel 工作簿：翻译共享字符串表，所有工作表保持一致 | 翻译副本 |
| `txt` | `.txt` | 纯文本小说，每行一个单元 | `<name>.<dst>.txt` |
| `md` | `.md` `.markdown` | Markdown：跳过代码块，标题/列表/引用前缀保留 | `<name>.<dst>.md` |
| `srt` / `vtt` | 文件 | 字幕：时间轴行与头部原样保留，字幕文本翻译 | 翻译副本 |
| `ass` | `.ass` `.ssa` | ASS/SSA 字幕：`{\tag}` 覆盖与 `\N` 换行保留，Name → 说话人 | 翻译副本 |
| `lrc` | `.lrc` | 歌词：时间戳保留，`[ti:…]` 元标签跳过 | 翻译副本 |
| `csv` | `.csv` `.tsv` | 表格（RFC4180：引号、内嵌换行）；只重写已翻译的记录 | 翻译副本 |
| `po` | `.po` `.pot` | Gettext：填充 `msgstr`；复数条目与头部直通 | 翻译副本 |
| `renpy` | `.rpy` | Ren'Py `translate` 块：对白 + `old`/`new` 字符串 | 翻译副本 |
| `rmmz` | 目录 | RPG Maker MV/MZ 数据 + `js/plugins.js` 中的插件参数（插件*源文件*永不修改） | 原地 + `*.attxbak` |
| `mtool` | `.json` | MTool `ManualTransFile.json`（内容嗅探） | 翻译副本 |
| `paratranz` | `.json` | Paratranz 导出；只填空的 `translation` 字段 | 翻译副本 |
| `vnt` | `.json` | VNTextPatch 导出（`name`/`message`） | 翻译副本 |
| `i18next` | `.json` | 字符串叶子的嵌套 JSON（≥80%） | 翻译副本 |
| `jsonl` | 文件/目录 | 通用逃生舱：通过外部提取/写回脚本支持任意引擎 | `translated.jsonl` |
| `custom:<name>` | 文件/目录 | **自定义 Profile**：agent（或你）为任何未知文本/JSON 格式编写的 TOML 规则 | 副本或原地 |

`attx formats` 以 JSON 打印这份清单（含已保存的自定义 Profile）。四种 `.json` 变体按内容嗅探区分；有歧义时用 `--engine <id>` 强制。

文本输入自动检测编码（通过 chardetng 检测 UTF-8 / UTF-16 BOM / Shift-JIS / GBK）；输出一律 UTF-8。

暂不支持（欢迎贡献适配器，见 [贡献](#贡献)）：Translator++ 工程、PDF、二进制封包（请走 JSONL 逃生舱）。

### 未知格式？教 attx 一个 Profile

`detect` 失败时不要停下来 —— attx 自带一套为 agent 打造的分析工具链：

```bash
attx analyze --input ./project         # recon: encoding, structure, samples, JSON shape
attx profile new --output fmt.toml     # documented rule template (line_regex / json_keys / json_paths)
attx profile test --profile fmt.toml --input ./project --roundtrip   # iterate until units look right
attx init --input ./project --profile fmt.toml --src ja --dst zh     # then extract/translate/writeback as usual
attx profile save --profile fmt.toml   # "remember this format" — detect auto-recognizes it from now on
```

Profile 是一个小 TOML 文件：带命名 `text`/`role` 组的逐行正则，和/或 JSON 键/路径选择器。完整的 agent 流程见 `profiles/examples/`（KiriKiri KAG、INI、通用 JSON）与 `skills/attx/references/custom-format-discovery.md`。

---

## 术语表

分批次翻译长篇作品的模型无法与自身保持一致：同一个专有名词会在不同章节间漂移。术语表为整部作品的每个术语固定一个约定译名。

**默认关闭** —— 构建术语表会花费额外的 LLM 调用。默认提取方式为 **`llm`**（模型读源文并给出术语）；**`stats`** 保留旧的先正则挖掘再命名的路径（更便宜）。

```bash
attx glossary build --workspace .attx --dry-run              # size the run, spend nothing
attx glossary build --workspace .attx                        # default method=llm
attx glossary build --workspace .attx --method stats         # regex mine + name
attx glossary list --workspace .attx
attx glossary add --workspace .attx --src アレイ --dst 艾蕾 --info "female given name"
attx glossary import --workspace .attx --file terms.json
attx glossary check --workspace .attx             # terms the translation ignored
attx review --workspace .attx                     # 残留假名、原文照抄、丢失保护码、姓名栏漂移
```

两种方式：

```
llm   (default): source batches → model emits {src,dst,info} → vote / max_terms → inject → check
stats:           mine (regex) → min_occurrences → max_terms → name (LLM) → inject → check
```

- **`llm`**：费用跟文本批次数走；召回更好；每个 `src` 必须是源文的真实子串（防幻觉）。
- **`stats`**：挖掘/阈值免费，模型只给高频命中命名 —— **费用跟术语数走**；`min_occurrences` 调低会收集更多、费用更高。

统计无法区分专有名词与普通词，所以 stats 给模型一个 `keep` 否决权；llm 依靠类型引导加上子串闸门。已决定的条目（包括否决的）会被记住。术语按批次注入，只注入该批次实际包含的术语（受 `inject_limit` 上限约束）。

每条目带一个消歧 `info`（“女性名字”、“地点”）。这不是装饰：没有它，模型无法判断名字在语境中应如何称呼。

在 `setting.toml` 中：

```toml
[glossary]
enabled = false        # build during `attx run`
method = "llm"         # llm | stats
min_occurrences = 10   # stats only
max_terms = 200        # cap on terms kept
inject_limit = 30      # cap on terms injected into one batch
```

显式执行 `attx glossary build` 会忽略 `enabled` —— 主动要求就是同意。而且一旦存在 `glossary.toml`，`translate` 总会从中注入：注入几乎免费，所以*不*使用你已经构建好的术语表反而奇怪。

---

## 自我改进的经验层

适配器用硬编码启发式决定提取什么，而这些表有时是错的 —— 一个看起来像 UI 文本的字段可能实际上是脚本逐字引用的标识符。翻译它，运行时就会出问题。此前这类修复只留在源码里，于是下一个项目又重新踩一遍。

attx 把这种判断作为数据保存，并**自动**捕获：每次成功的 `writeback` 都会把运行总结为经验条目，零 API 成本，因为证据已经躺在工作区数据库里。

```bash
attx writeback --workspace .attx         # …and learn from the run, automatically
attx writeback --workspace .attx --no-learn   # opt out for one run
attx learn summarize --workspace .attx   # or trigger it by hand
attx learn summarize --workspace .attx --llm  # also ask the model (costs money)
attx learn pending                       # entries awaiting approval, with evidence
attx learn review --approve 1,3          # approve; only now do they delete anything
attx learn list                          # what is active
attx learn defaults --format rmmz        # example: built-in baseline for one format
attx learn forget --field achievename    # drop one
attx extract --no-knowledge              # escape hatch: ignore all of it
```

**文件格式刻意保持开放。** 条目带 `kind`，attx 不认识的 kind 会原样往返 —— 所以 agent 可以发明 `kind = "voice-hint"`，attx 会原封不动地交还，而不是悄悄丢弃。目前有两种 kind 会被处理：

```toml
[[entry]]
kind = "field"          # a field-name extraction judgement
field = "key"
verdict = "skip"        # skip | extract
scope = "nested"        # nested | top | any
domain = "plugins"      # restrict to one unit domain; empty = any
status = "pending"      # approved | pending

[[entry]]
kind = "note"           # free-form experience; topic="prompt" reaches the model
topic = "prompt"
text = "This format loses control codes; keep every [CTRL_n] verbatim."
```

四层经验合并，后者优先：内置默认（内嵌，见 `learn defaults`）→ `$ATTX_HOME/knowledge/<format>.toml` → `<workspace>/experience.toml`。同一层内，精确字段名胜过 `*后缀`，`skip` 胜过 `extract`。

三个值得知道的保护机制：

- **新增自动生效；删除等你批准。** Note 与 `extract` 条目立即生效 —— 最坏情况只是提示词变长。`skip` 是唯一会删除文本的判定，所以它先记为 `pending`，在 `learn review --approve` 之前不做任何事。漏译在 `status` 中可见；被悄悄丢弃的行则不可见。
- **学习可以覆盖名称启发式，但绝不会覆盖值的证据。** 当值是数字、路径或脚本时，`extract` 条目会被拒绝，因此坏条目不可能把开关 id 或文件名发给模型。
- **条目按域限定作用范围。** 某个域的规则不会在另一个同名但含义不同的域上触发。

---

## CLI 参考

| 命令 | 作用 |
|---------|------|
| `doctor [--ping] [--json]` | 配置检查 / LLM 连通性检查 |
| `formats` | 支持的适配器 + 已保存的 Profile（JSON） |
| `detect --input <path>` | 格式探测，含已保存的 Profile（保留 `--game` 别名） |
| `analyze --input <path>` | 未知输入的侦察报告（编码、结构、样本） |
| `profile new/test/save/list` | 编写、迭代并记住自定义格式 Profile |
| `init --input <path> --src --dst [--profile]` | 创建工作区 + SQLite |
| `extract --workspace [--no-knowledge]` | 适配器 → 文本单元 |
| `translate --workspace [--limit] [--dry-run] [--retry-passthrough]` | 对待译单元调用 LLM，增量保存 |
| `writeback --workspace [--dry-run] [--no-learn]` | 渲染翻译输出；除非选择退出，否则捕获经验 |
| `run --input …` | init + extract +（术语表）+ translate + writeback |
| `status --workspace` | 计数（含 passthrough）+ 按域细分 |
| `translate-jsonl` / `export-jsonl` / `import-jsonl` | 交换（`--filter` 含 `passthrough`） |
| `learn summarize/pending/review/list/defaults/forget` | 自我改进：积累提取经验 |
| `glossary build/list/add/remove/import/export/check` | 整部作品一致的专有名词译名 |

全局：`--config /path/to/setting.toml`（默认 `./setting.toml` 或 `$ATTX_HOME/setting.toml`）；`--client <name>` 选择非默认的 LLM 客户端。

每个命令都在 stdout 报告机器可读的 JSON；错误以非零退出码输出到 stderr。确切的 JSON 结构固定在 `skills/attx/references/cli-command-contract.md`。

---

## 文档

长文档（EN / 中文 / 日本語）：**https://kaecho.github.io/attx/**

---

## 贡献

欢迎 PR —— 尤其是新的格式适配器。代码库刻意保持小而朴素；请保持这样。

### 架构

```
src/
  main.rs          CLI
  pipeline.rs      init / extract / translate / writeback / run
  adapter/         one module per format (+ custom profiles)
  llm.rs           OpenAI-compatible client, batching, masking
  store.rs         SQLite workspace
  knowledge.rs     experience layers (learn)
  glossary.rs      proper-noun glossary
  profile.rs       custom format profiles
```

### 添加新格式适配器

1. 在 `src/adapter/<name>.rs` 实现 `FormatAdapter`（`detect` / `extract` / `writeback`）。
2. 在 `src/adapter/mod.rs` 注册（顺序 = 检测优先级）。
3. 加一个带小 fixture 的往返单元测试。
4. 在 `attx formats` 输出与本 README 中记录该 id。

### PR 检查清单

- [ ] `cargo test` 通过
- [ ] 仓库中不含 API Key 或受版权保护的示例文本
- [ ] 新适配器：detect 不会在其他格式上误报
- [ ] 对用户可见的 README / `formats` 已更新

### 路线图（认领一个）

- 更多文档 / 游戏 / 本地化适配器
- 更丰富的自定义 Profile 原语
- 面向非 CLI 主机的可选 MCP 封装

---

## 许可证

MIT
