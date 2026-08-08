# attx

[English](README.md) | **中文** | [文档](https://emptysuns.github.io/attx/zh/)

**Agent Translation Toolkit eXtensible** — 纯 Rust 单二进制、格式无关的 AI 翻译框架，面向 Agent 与人类。

```
extract（格式适配器）→ translate（LLM 核心）→ writeback（格式适配器）
```

用任意 OpenAI 兼容 LLM 翻译电子书、文档、字幕、本地化文件、游戏等。进度缓存在 SQLite 工作区，中断后可免费续跑。格式支持参考 [AiNiee](https://github.com/NEKOparapa/AiNiee) 的读写插件集，以 Rust 适配器重写。

---

## Agent 快速上手（推荐）

attx 的设计目标：编码 Agent **读完 Skill → 问答收集配置 → 写入 `setting.toml` → 跑完整流水线**。你通常不必先手改配置。

### 1. 安装二进制

- **发行包：** [Releases](https://github.com/emptysuns/attx/releases)（`v*` 标签）
- **源码：**

```bash
git clone https://github.com/emptysuns/attx.git
cd attx
cargo build --release
./target/release/attx --help
# 可选:
cargo install --path .
```

### 2. 安装 Skill（让 Agent 按协议执行）

```text
skills/attx/SKILL.md           # 阶段、硬停止、问答式配置向导
skills/attx/references/        # CLI 契约、开局、未知格式发现、故障恢复、JSONL、反馈
```

**Claude Code：**

```bash
# 个人全局（所有会话）:
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
# 或项目级:
mkdir -p .claude/skills && cp -a skills/attx .claude/skills/
```

**其他 Agent**（Cursor / Codex / OpenCode / …）：保留仓库路径，在对话里声明：

```text
严格遵循 <attx目录>/skills/attx/SKILL.md
```

**为什么是 Skill 而不是 MCP？** attx 是本地 CLI、stdout 全 JSON——这就是编码类 Agent 的原生工具面。Skill 是纯 Markdown；MCP 只是把同一 CLI 再包一层常驻进程。

### 3. 一条提示词：Agent 问答配置后直接翻译

`setting.toml` 缺失或 `attx doctor` 失败时，Skill **强制**走交互向导。Agent 逐项询问：

1. API 端点（OpenAI / DeepSeek / 自定义 OpenAI 兼容 `base_url`）
2. API Key → 只写入 `setting.toml`，**绝不回显**到对话
3. 模型名
4. 语向（`src` / `dst`）
5. 可选：并发、术语表

然后执行 `attx doctor --ping`，再进入流水线。

可复制：

```text
请使用 attx 工具包（目录：<attx目录>），严格遵循 skills/attx/SKILL.md。

若尚未配置，先用问答向导帮我写好 setting.toml（端点、Key、模型、语向），
再把 <输入路径> 从日文翻译为简体中文。

约束：
1. 只通过 attx CLI 操作；禁止手改输入文件、attx.db、工具源码。
2. 未配置模型时先走问答向导；不要把 API Key 打进对话记录。
3. doctor --ping → detect → init → extract → status → translate --limit 20 → 全量。
4. 文件类优先产出翻译副本；任何原地覆盖写回前必须问我。
5. 每阶段汇报：做了什么、status 数字、下一步。
```

更短也可以：

```text
帮我配置 attx，然后把 ./novel.epub 从日文翻译成简体中文。
```

---

## 配置 LLM（手动方式）

只在你想自己改文件、不走 Agent 向导时需要。

```bash
cp setting.example.toml setting.toml
```

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"          # OpenAI 兼容 Chat Completions
base_url = "https://your-provider.example/v1"
api_key = "YOUR_API_KEY"
model = "your-model-name"
timeout = 600

[translation]
worker_count = 8       # 并行 HTTP 批次数
rpm = 60               # 全局请求限速（次/分钟），0 = 不限
retry_count = 3
retry_delay = 2
batch_chars = 2500     # 每批最大源文字符数
max_context_items = 6  # 每批最大条目数
```

`setting.toml` 已被 gitignore——永远不要提交 API Key。用 `attx doctor --ping` 验证连通。

查找顺序：`--config` → `./setting.toml` → `$ATTX_HOME/setting.toml`。

---

## 使用

### 一键

```bash
attx run --input "novel.epub" --src ja --dst zh
# → 在输入旁生成 novel.zh.epub；原文件永不修改
```

### 分步（大输入推荐，先试译 20 条）

```bash
attx detect  --input book.epub
attx init    --input book.epub --src ja --dst zh      # 工作区: .attx-book/
attx extract --workspace .attx-book
attx status  --workspace .attx-book
attx translate --workspace .attx-book --limit 20      # 试译
attx translate --workspace .attx-book                 # 全量；中断后重跑续译
attx writeback --workspace .attx-book                 # → book.zh.epub
```

多数文件格式产出**翻译副本**（`*.<dst>.*`），不碰原件。少数目录型适配器可能**原地写回**并生成 `*.attxbak`——请先 `writeback --dry-run`，并由 Agent 在覆盖前征求确认。

真实验证：一本 4171 段、10.9 MB 含插图的轻小说 EPUB 整卷 ja→zh 一次跑完——覆盖率 100%，EPUB 结构/插图完好，目录与 `dc:title`/`dc:language` 均已本地化。

### 人工审校 / 离线通道（JSONL）

```bash
attx export-jsonl --workspace .attx-book --output pending.jsonl --filter pending
# 外部编辑 translation_lines 后:
attx import-jsonl --workspace .attx-book --input pending.jsonl
attx writeback    --workspace .attx-book
```

独立模式（无工作区）：

```bash
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh
```

### CLI 参考

| 命令 | 作用 |
|------|------|
| `doctor [--ping] [--json]` | 配置检查 / LLM 连通性 |
| `formats` | 适配器 + 已存 Profile 能力清单（JSON） |
| `detect --input <路径>` | 格式探测，含已存 Profile（保留 `--game` 别名） |
| `analyze --input <路径>` | 未知输入侦察报告（编码/结构/样本） |
| `profile new/test/save/list` | 编写、迭代、保存自定义格式 Profile |
| `init --input <路径> --src --dst [--profile]` | 建工作区 + SQLite |
| `extract --workspace` | 适配器 → 文本单元 |
| `translate --workspace [--limit] [--dry-run] [--retry-passthrough]` | 翻译 pending，批次增量落库 |
| `writeback --workspace [--dry-run] [--no-learn]` | 产出译文文件；顺带自动沉淀经验（可关） |
| `run --input …` | init + extract +（术语表）+ translate + writeback |
| `status --workspace` | 进度统计（含 passthrough 与按 domain 细分） |
| `translate-jsonl` / `export-jsonl` / `import-jsonl` | 数据交换（`--filter` 支持 `passthrough`） |
| `learn summarize/pending/review/list/defaults/forget` | 自我改进：积累提取经验 |
| `glossary build/list/add/remove/import/export/check` | 术语表：全作品统一专有名词译名 |

全局：`--config /path/to/setting.toml`（默认 `./setting.toml` 或 `$ATTX_HOME/setting.toml`）；`--client <名>` 选用非默认 LLM client。

模型拒答/反复失败的条目会以 **passthrough 占位**（保留原文并打标）让整轮跑完；`status` 会报数，`translate --retry-passthrough` 可只重试这些条目。

### 自我改进的经验层

适配器靠硬编码启发式判断该提取什么，而这些表有时是错的——字段名看着是文案，值却可能是脚本按原文引用的标识符。翻译它，运行时就会坏。以前每次修复只留在源码里，下一个项目重新踩一遍。

attx 把这类判断变成数据，并且**自动**沉淀：每次 `writeback` 成功后都会把本轮运行总结成经验条目，**零 API 成本**——证据本来就躺在工作区数据库里。

```bash
attx writeback --workspace .attx         # …并自动从本轮运行中学习
attx writeback --workspace .attx --no-learn   # 本次不学
attx learn summarize --workspace .attx   # 或手动触发
attx learn summarize --workspace .attx --llm  # 额外让模型复核（有费用）
attx learn pending                       # 待批准条目（含证据与样本值）
attx learn review --approve 1,3          # 批准；此时才会真的删东西
attx learn list                          # 当前生效的经验
attx learn defaults --format rmmz        # 示例：打印某格式内置基线
attx learn forget --field achievename    # 撤销某条
attx extract --no-knowledge              # 逃生舱：忽略全部经验
```

**文件格式刻意是开放式的。** 每条 entry 带一个 `kind`，attx 不认识的 kind 会**原样往返保留**——agent 可以自创 `kind = "voice-hint"`，attx 会原封不动还给它，而不是静默丢弃。目前 attx 会执行的有两种：

```toml
[[entry]]
kind = "field"          # 字段名级提取判断
field = "key"
verdict = "skip"        # skip | extract
scope = "nested"        # nested | top | any
domain = "plugins"      # 限定单元 domain；留空表示不限
status = "pending"      # approved | pending

[[entry]]
kind = "note"           # 自由经验；topic="prompt" 的会进模型提示词
topic = "prompt"
text = "该格式的译文容易丢控制码，务必原样保留每个 [CTRL_n]。"
```

四层合并，后者覆盖前者：内置默认（内嵌，见 `learn defaults`）→ `$ATTX_HOME/knowledge/<格式>.toml` → `<工作区>/experience.toml`。同层内，精确字段名胜过 `*后缀`，`skip` 胜过 `extract`。

三条安全保障：

- **加法自动生效，减法等你点头。** note 与 `extract` 立即生效——最坏是提示词长几句。`skip` 是唯一会删文本的判定，所以写入时为 `pending`，在 `learn review --approve` 之前什么也不做。漏译在 `status` 里看得见，静默丢掉的那行看不见。
- **学习可以推翻字段名启发式，但不能推翻值本身的证据。** 当值是数字、路径或脚本时，`extract` 条目会被拒绝——坏条目不可能把开关 ID、文件名送去翻译。
- **条目受 domain 约束。** 某一域的规则不会误伤另一域里同名字段。

### 术语表

模型分批翻译长篇时无从与自己保持一致：同一专有名词会在不同章节漂移。术语表为整部作品钉死统一译名。

**默认关闭**——构建会产生额外 LLM 费用。

```bash
attx glossary build --workspace .attx --dry-run   # 先看候选，不花钱
attx glossary build --workspace .attx             # 挖掘 → 卡阈值 → 交模型命名
attx glossary list --workspace .attx
attx glossary add --workspace .attx --src アレイ --dst 艾蕾 --info 女性名字
attx glossary import --workspace .attx --file terms.json
attx glossary check --workspace .attx             # 译文里没生效的术语
```

流程是统计优先的，这正是它便宜的原因：

```
挖掘（正则，免费） → min_occurrences → max_terms → 命名（LLM） → 按批注入 → 回检
```

挖掘与卡阈值都不花钱，所以模型只会被问到足够高频的词——**LLM 花费只与术语数相关，与作品体量无关**。调低 `min_occurrences` 收录更多术语、花更多钱，这就是唯一的杠杆。

统计分不清专有名词和常见词，所以模型有 `keep` 标志可否决候选，且否决会被记住。术语按批注入，只注入该批真实出现的那些。

每条术语带消歧描述 `info`（「女性名字」「地点」）——没有它，模型很难判断称呼与语境。

在 `setting.toml` 里：

```toml
[glossary]
enabled = false        # 是否在 `attx run` 中构建
min_occurrences = 10
max_terms = 200
inject_limit = 30

[learn]
auto_summarize = true  # writeback 后沉淀经验（免费）
llm_review = false     # 额外让模型复核提案（有费用）
```

显式 `attx glossary build` 不受 `enabled` 限制——主动要求即同意。一旦 `glossary.toml` 存在，`translate` 就一律注入。

---

## 支持的格式

| id | 输入 | 说明 | 输出 |
|----|------|------|------|
| `epub` | `.epub` | 电子书/轻小说（段落级，保留插图与排版，rt 注音剔除） | `<名>.<目标语言>.epub` |
| `html` | `.html/.htm/.xhtml` | 单页 HTML（块级 + `<title>`） | 同名副本 |
| `docx` | `.docx` | Word 文档 | `<名>.<目标语言>.docx` |
| `xlsx` | `.xlsx/.xlsm` | Excel（译 sharedStrings，全表一致） | 同名副本 |
| `txt` / `md` | 文件 | 小说纯文本 / Markdown（跳过代码块，保留语法前缀） | 同名 `.zh.txt` 等 |
| `srt` / `vtt` / `lrc` | 文件 | 字幕/歌词（时间轴原样保留） | 同名副本 |
| `ass` | `.ass/.ssa` | ASS/SSA（`{\tag}` 与 `\N` 保留，Name 作角色） | 同名副本 |
| `csv` | `.csv/.tsv` | 表格（RFC4180） | 同名副本 |
| `po` | `.po/.pot` | Gettext（填 msgstr） | 同名副本 |
| `renpy` | `.rpy` | Ren'Py `translate` 块 | 同名副本 |
| `rmmz` | 目录 | RPG Maker MV/MZ（data + plugins.js 参数；不改插件源码） | 原地写回 + `*.attxbak` |
| `mtool` / `paratranz` / `vnt` / `i18next` | `.json` | 内容嗅探的本地化 JSON | 同名副本 |
| `jsonl` | 文件/目录 | 通用逃生舱 | `translated.jsonl` |
| `custom:<名>` | 文件/目录 | **自定义 Profile**（TOML，Agent 可编写并保存） | 副本或原地 |

`attx formats` 输出机器可读清单（含已保存 Profile）。四种 `.json` 靠内容嗅探区分；歧义时 `--engine <id>` 强制。

文本编码自动检测（UTF-8 / Shift-JIS / GBK / UTF-16 BOM），输出一律 UTF-8。

尚未支持（欢迎贡献）：Translator++ 工程、PDF、二进制封包（走 JSONL 逃生舱）。

### 遇到不支持的格式？教会 attx 一个 Profile

`detect` 失败时不要停——attx 自带面向 Agent 的发现工具链：

```bash
attx analyze --input ./project
attx profile new --output fmt.toml
attx profile test --profile fmt.toml --input ./project --roundtrip
attx init --input ./project --profile fmt.toml --src ja --dst zh
attx profile save --profile fmt.toml
```

Profile 是小 TOML：按行正则（`text`/`role` 命名组）和/或 JSON 键路径。见 `profiles/examples/` 与 `skills/attx/references/custom-format-discovery.md`。

---

## 文档

完整说明（中 / 英 / 日）：**https://emptysuns.github.io/attx/zh/**

---

## 贡献指南

欢迎 PR——尤其是新格式适配器。代码库刻意保持小而朴素，请维持这一点。

### 架构

```
src/
  main.rs          CLI
  pipeline.rs      init / extract / translate / writeback / run
  adapter/         各格式适配器（+ 自定义 Profile）
  llm.rs           OpenAI 兼容客户端、批处理、控制符掩码
  store.rs         SQLite 工作区
  knowledge.rs     经验层（learn）
  glossary.rs      术语表
  profile.rs       自定义格式 Profile
```

### 添加新格式适配器

1. 在 `src/adapter/<name>.rs` 实现 `FormatAdapter`（`detect` / `extract` / `writeback`）。
2. 在 `src/adapter/mod.rs` 注册。
3. 用最小 fixture 做 round-trip 单测。
4. 更新 `attx formats` 与本 README。

### PR 检查单

- [ ] `cargo test` 通过
- [ ] 仓库中无 API Key、无受版权保护的样例正文
- [ ] 新适配器：detect 不对其他格式误报
- [ ] 用户可见变更已更新 README / formats

### Roadmap（欢迎认领）

- 更多文档 / 游戏 / 本地化适配器
- 更丰富的自定义 Profile 原语
- 可选的 MCP 包装（非 CLI 宿主）

---

## 许可证

MIT
