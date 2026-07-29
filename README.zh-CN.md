# attx

[English](README.md) | **中文**

**Agent Translation Toolkit eXtensible** —— 纯 Rust 单二进制、格式无关的 AI 翻译框架,同时面向 AI Agent 与人类使用。

```
提取 extract(格式适配器) → 翻译 translate(LLM 核心) → 写回 writeback(格式适配器)
```

用任意 OpenAI 兼容模型翻译游戏、电子书、文档、字幕与本地化文件。进度缓存在 SQLite 工作区,中断后重跑自动续译。格式覆盖对标 [AiNiee](https://github.com/NEKOparapa/AiNiee) 的 Reader/Writer 插件集,以 Rust 适配器重新实现。

## 支持的格式

| id | 输入 | 说明 | 输出 |
|----|------|------|------|
| `rmmz` | 目录 | RPG Maker MV/MZ:`data/*.json` 事件/系统/数据库 + `js/plugins.js` 插件参数(不改插件源码) | 原地写回 + `*.attxbak` |
| `epub` | `.epub` | 电子书/轻小说:段落级提取,`<rt>` 注音剔除,插图与排版保留,`dc:language` 更新 | `<名>.<目标>.epub` |
| `html` | `.html` `.htm` `.xhtml` | 单页 HTML:块级 + `<title>` | 翻译副本 |
| `docx` | `.docx` | Word 文档:按 `w:t` 运行块的段落级翻译 | `<名>.<目标>.docx` |
| `xlsx` | `.xlsx` `.xlsm` | Excel:翻译 sharedStrings 共享字符串表,全部工作表一致生效 | 翻译副本 |
| `txt` | `.txt` | 纯文本小说,一行一单元 | `<名>.<目标>.txt` |
| `md` | `.md` | Markdown:跳过代码块,保留标题/列表/引用前缀 | `<名>.<目标>.md` |
| `srt` / `vtt` | 文件 | 字幕:时间轴与头部原样,只译台词 | 翻译副本 |
| `ass` | `.ass` `.ssa` | ASS/SSA 字幕:`{\tag}` 特效与 `\N` 换行保留,Name 字段作角色 | 翻译副本 |
| `lrc` | `.lrc` | 歌词:时间戳保留,`[ti:…]` 元标签跳过 | 翻译副本 |
| `csv` | `.csv` `.tsv` | 表格(RFC4180:引号/内嵌换行;只重写有译文的记录) | 翻译副本 |
| `po` | `.po` `.pot` | Gettext:填充 `msgstr`;复数条目与头部原样 | 翻译副本 |
| `renpy` | `.rpy` | Ren'Py `translate` 块:对白 + `old`/`new` 字符串 | 翻译副本 |
| `mtool` | `.json` | MTool `ManualTransFile.json`(内容嗅探) | 翻译副本 |
| `paratranz` | `.json` | Paratranz 导出;只填空的 `translation` | 翻译副本 |
| `vnt` | `.json` | VNTextPatch 导出(`name`/`message`) | 翻译副本 |
| `i18next` | `.json` | 字符串叶子占 ≥80% 的嵌套 JSON | 翻译副本 |
| `jsonl` | 文件/目录 | 万能逃生舱:任何引擎外部导出/写回 | `translated.jsonl` |
| `custom:<名>` | 文件/目录 | **自定义 Profile**:用 TOML 规则描述任意文本/JSON 格式(agent 可自行编写) | 副本或原地 |

`attx formats` 输出机器可读清单(含已保存的自定义 Profile)。四种 `.json` 按内容嗅探区分,歧义时用 `--engine <id>` 强制。

文本类输入**编码自动检测**(UTF-8 / Shift-JIS / GBK / UTF-16 BOM),输出一律 UTF-8。

暂不支持(欢迎贡献适配器,见[贡献指南](#贡献指南)):Translator++ 工程、PDF、二进制封包(走 JSONL 逃生舱)。

### 遇到不支持的格式?教会 attx 一个 Profile

`detect` 失败时不必放弃——attx 内置一套给 agent 用的格式发现工具链:

```bash
attx analyze --input ./game            # 侦察:编码、结构、样本、JSON 形状
attx profile new --output fmt.toml     # 带注释的规则模板(line_regex / json_keys / json_paths)
attx profile test --profile fmt.toml --input ./game --roundtrip   # 迭代到单元/样本正确(不写盘)
attx init --input ./game --profile fmt.toml --src ja --dst zh     # 之后 extract/translate/writeback 照常
attx profile save --profile fmt.toml   # "记住这个格式"——今后 detect 自动识别
```

Profile 是一个小 TOML:行级正则(命名组 `text`/`role`)和/或 JSON 键/路径选择器。
样例见 `profiles/examples/`(KiriKiri KAG、INI、通用 JSON),完整 agent 工作流见
`skills/attx/references/custom-format-discovery.md`。

---

## 安装

### Release 二进制

从 [Releases](https://github.com/emptysuns/attx/releases) 下载(`v*` 标签)。

### 源码编译

```bash
git clone https://github.com/emptysuns/attx.git
cd attx
cargo build --release
./target/release/attx --help
# 可选:
cargo install --path .
```

---

## 配合 AI Agent 使用(Skill)

attx 自带一份**执行 Skill**——Agent 照协议走,而不是即兴发挥:

```text
skills/attx/SKILL.md           # 阶段、硬停止、问答式配置向导
skills/attx/references/        # CLI 契约、开局方式、未知格式发现、故障恢复、JSONL、反馈迭代
```

**为什么做成 Skill 而不是 MCP?** attx 是本地 CLI、stdout 全 JSON——这本身就是
编码类 agent 的原生工具面,零额外基础设施。Skill 是纯 markdown,任何 agent 都能照做
(Claude Code / Cursor / Codex / OpenCode…);MCP 只是把同一个 CLI 再包一层常驻进程。
如果某个非 CLI 客户端确实需要 MCP,由于每条命令都输出 JSON,包一层也非常容易。

### 安装 Skill

**Claude Code**(推荐):

```bash
# 个人全局(所有会话生效):
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
# 或项目级(仅当前项目):
mkdir -p .claude/skills && cp -a skills/attx .claude/skills/
```

之后 Agent 读取 skill 列表即可发现 attx 并自动路由翻译请求(也可 `/attx` 显式调用)。

**其他 Agent**(Cursor / Codex / OpenCode / …):保留仓库检出,在对话里声明:

```text
严格遵循 <attx目录>/skills/attx/SKILL.md
```

### 问答式配置

不需要手工编辑配置就能开始。`setting.toml` 缺失或失效时,Skill 会让 Agent 走**交互式向导**:逐项询问 API 端点(OpenAI / DeepSeek / 自建中转)、API Key(直接写入 `setting.toml`,绝不回显)、模型名、语向、并发数,然后用 `attx doctor --ping` 验证。直接对 Agent 说:

```text
帮我配置 attx,然后把 ./novel.epub 从日文翻译成简体中文。
```

### Agent 开场提示词示例

```text
请使用 attx 工具包(目录:<attx目录>)按 skills/attx/SKILL.md 流程,
把 <输入文件或游戏目录> 从日文翻译为简体中文。

约束:
1. 只通过 attx CLI 操作;禁止手改输入文件、attx.db、工具源码。
2. 若未配置模型,先用问答向导帮我配置;不要把 API Key 打进对话记录。
3. 先 doctor --ping、detect、init、extract、status;再 limit 20 试译;通过后全量。
4. RPG Maker 原地写回前必须问我;文档类直接产出翻译副本即可。
5. 每阶段结束汇报:做了什么、status 数字、下一步。
```

---

## 配置 LLM(手动方式)

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
rpm = 60               # 全局请求限速(次/分钟),0 = 不限
retry_count = 3
retry_delay = 2
batch_chars = 2500     # 每批最大源文字符数
max_context_items = 6  # 每批最大条目数
```

`setting.toml` 已被 gitignore——永远不要提交 API Key。用 `attx doctor --ping` 验证连通。

---

## 使用

### 翻译文档 / 电子书 / 字幕

```bash
attx run --input "novel.epub" --src ja --dst zh
# → 在输入旁生成 novel.zh.epub;原文件永不修改
```

分步执行(大输入推荐,先试译 20 条):

```bash
attx detect  --input book.epub
attx init    --input book.epub --src ja --dst zh      # 工作区: .attx-book/
attx extract --workspace .attx-book
attx status  --workspace .attx-book
attx translate --workspace .attx-book --limit 20      # 试译
attx translate --workspace .attx-book                 # 全量;中断后重跑续译
attx writeback --workspace .attx-book                 # → book.zh.epub
```

真实验证:一本 4171 段、10.9 MB 含插图的轻小说 EPUB 整卷 ja→zh 一次跑完——覆盖率 100%,EPUB 结构/插图完好,目录与 `dc:title`/`dc:language` 均已本地化。

### 翻译 RPG Maker MV/MZ 游戏

```bash
attx run --input /path/to/game --src ja --dst zh --no-writeback
attx writeback --workspace /path/to/game/.attx --dry-run   # 预览
attx writeback --workspace /path/to/game/.attx             # 原地写回 + *.attxbak
```

### 人工审校 / 离线通道(JSONL)

```bash
attx export-jsonl --workspace .attx-book --output pending.jsonl --filter pending
# 外部编辑 translation_lines 后:
attx import-jsonl --workspace .attx-book --input pending.jsonl
attx writeback    --workspace .attx-book
```

独立模式(无工作区):

```bash
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh
```

### CLI 参考

| 命令 | 作用 |
|------|------|
| `doctor [--ping] [--json]` | 配置检查 / LLM 连通性 |
| `formats` | 适配器 + 已存 Profile 能力清单(JSON) |
| `detect --input <路径>` | 格式探测,含已存 Profile(保留 `--game` 别名) |
| `analyze --input <路径>` | 未知输入侦察报告(编码/结构/样本) |
| `profile new/test/save/list` | 编写、迭代、保存自定义格式 Profile |
| `init --input <路径> --src --dst [--profile]` | 建工作区 + SQLite |
| `extract --workspace` | 适配器 → 文本单元 |
| `translate --workspace [--limit] [--dry-run] [--retry-passthrough]` | 翻译 pending,批次增量落库 |
| `writeback --workspace [--dry-run]` | 产出译文文件 |
| `run --input …` | init + extract + translate + writeback |
| `status --workspace` | 进度统计(含 passthrough 与按 domain 细分) |
| `translate-jsonl` / `export-jsonl` / `import-jsonl` | 数据交换(`--filter` 支持 `passthrough`) |
| `learn scan/pending/review/list/forget` | 自我改进:从证据中学习提取规则 |

全局:`--config /path/to/setting.toml`(默认 `./setting.toml` 或 `$ATTX_HOME/setting.toml`);`--client <名>` 选用非默认 LLM client。

模型拒答/反复失败的条目会以**passthrough 占位**(保留原文并打标)让整轮跑完;
`status` 会报数,`translate --retry-passthrough` 可只重试这些条目。

### 自我改进的提取层

适配器靠硬编码启发式判断该提取什么,而这些表有时是错的——像 `AchieveName`
这种字段名看着是文本,值却是事件脚本按原文引用的标识符。翻译它,成就就永远
解锁不了。以前每次修复只留在源码里,下一个游戏重新踩一遍。

`attx learn` 把这类判断变成可跨项目积累的数据:

```bash
attx learn scan --workspace .attx        # 挖掘证据,记录提案
attx learn scan --workspace .attx --llm  # 额外让模型复核提案
attx learn pending                       # 查看提案(含证据与样本值)
attx learn review --approve 1,3          # 批准;此时规则才生效
attx learn list                          # 当前生效规则
attx learn forget --field achievename    # 撤销某条
attx extract --no-knowledge              # 逃生舱:忽略全部已学规则
```

证据免费且客观,来自工作区里已经发生的事:提取出来却是机器字面量的值、
与原文完全相同的译文、以及 passthrough 聚集。统计按**字段名**聚合而非按单元——
一条 passthrough 是噪声,同一字段名下 6/6 就是信号。

规则按格式存为可读 TOML,位于 `$ATTX_HOME/knowledge/`(或
`~/.config/attx/knowledge/`),可以直接读、改、用 git 管、删。

两条值得知道的安全保障:

- **未批准不生效。** `scan` 只写提案,不碰提取行为。
- **学习可以推翻字段名启发式,但不能推翻值本身的证据。** 当值是数字、路径或
  脚本时,`extract` 规则会被拒绝——所以一条坏规则不可能把开关 ID、文件名送去翻译。

---

## 贡献指南

欢迎 PR——尤其是新格式适配器。这个代码库刻意保持小而朴素,请维持这一点。

### 架构

```
src/
  main.rs          CLI (clap)
  model.rs         TextUnit / Translation / 控制符掩码 / 语言探测
  config.rs        setting.toml
  store.rs         SQLite 工作区(单元、译文、hash 缓存、passthrough 标记)
  llm.rs           OpenAI 兼容 chat、分批、并行 worker、限速、按文体 profile 的 prompt
  quality.rs       行数 / 控制符完整性检查
  textio.rs        编码自动检测(UTF-8 / Shift-JIS / GBK / UTF-16)
  profile.rs       自定义格式 Profile(line_regex / json_keys / json_paths 规则)
  knowledge.rs     已学提取规则:模型、TOML 存储、对单元的纯函数过滤
  learn.rs         证据挖掘、提案、审核批准、可选 LLM 复核
  pipeline.rs      流程编排 + analyze(不含格式知识;适配器不碰网络)
  adapter/
    mod.rs         FormatAdapter trait + 注册表 + 共享工具
    xmllite.rs     无损迷你 XML 树(epub/docx/xlsx 共用)
    epub.rs docx.rs xlsx.rs plaintext.rs subtitle.rs ass.rs csv.rs po.rs renpy.rs
    jsonkv.rs rmmz.rs rmmz_plugins.rs jsonl.rs
profiles/examples/ 自定义 Profile 起步样例(KiriKiri KAG、INI、通用 JSON)
```

分层规则:**适配器只做解析/序列化**——分批、LLM 调用、缓存、重试、写盘都在 pipeline。适配器永远不发网络请求。

### 添加新格式适配器

1. 新建 `src/adapter/myformat.rs` 实现 trait:

```rust
pub trait FormatAdapter: Send + Sync {
    fn id(&self) -> &'static str;               // 稳定 id,用于 --engine 与 DB
    fn label(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str] { &[] } // 空 → 目录型输入
    fn detect(&self, input: &Path) -> Option<DetectHit>;    // 默认按扩展名
    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>>;
    fn writeback(&self, input: &Path, target_lang: &str,
                 units: &[TextUnit], translations: &BTreeMap<String, Translation>)
                 -> Result<Vec<OutputFile>>;    // 绝对路径 + 字节
}
```

2. 在 `src/adapter/mod.rs` 的 `all_adapters()` 注册(顺序即探测优先级;嗅探 `.json` 的适配器按"最具体的形状在前")。
3. 经验法则:
   - 只对 `needs_translation(text, source_lang)` 的文本产出单元。
   - `location` 必须是稳定且零填充的地址(如 `c00042`)——它既是写回锚点也是批次排序键。
   - `context` 决定哪些相邻单元进同一 LLM 批次(章节/文件/区段)。
   - 未翻译的单元写回时必须保持原样;文档格式输出**翻译副本**,绝不修改输入。
   - 需要模型保留的内联标记,用 `model::mask_controls` 掩码或扩展 `llm.rs` 系统提示词。
4. 在同文件写一个往返单测(`构造微型样本 → extract → 伪造译文 → writeback → 断言`),参考 `epub.rs` 的测试。
5. `cargo fmt && cargo clippy && cargo test` 全绿;同步更新两份 README 与 `skills/attx/SKILL.md` 的格式表。

### PR 检查单

- [ ] 每个新适配器都有往返测试(fixture 测试内构造,不往 git 放二进制)
- [ ] 不引入新依赖,除非手写实现明显更差
- [ ] `cargo fmt` / `cargo clippy` 干净,CI 全绿(Linux + Windows)
- [ ] diff 中没有 API Key、游戏数据样本或受版权保护的内容
- [ ] 两份 README 与 SKILL.md 的格式表已更新

### Roadmap(欢迎认领)

- Translator++(.trans)适配器
- 跨批次术语表 / 译名固定
- PDF(经外部工具,同 AiNiee 借助 BabelDOC 的方式)
- 可选输出编码(供只认 Shift-JIS 的老引擎)
- CLI 之上的 MCP server 封装(供非 CLI 客户端)

---

## 许可证

MIT
