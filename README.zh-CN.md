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
| `docx` | `.docx` | Word 文档:按 `w:t` 运行块的段落级翻译 | `<名>.<目标>.docx` |
| `txt` | `.txt` | 纯文本小说,一行一单元(需 UTF-8;旧编码先 `iconv`) | `<名>.<目标>.txt` |
| `md` | `.md` | Markdown:跳过代码块,保留标题/列表/引用前缀 | `<名>.<目标>.md` |
| `srt` / `vtt` | 文件 | 字幕:时间轴与头部原样,只译台词 | 翻译副本 |
| `lrc` | `.lrc` | 歌词:时间戳保留,`[ti:…]` 元标签跳过 | 翻译副本 |
| `po` | `.po` `.pot` | Gettext:填充 `msgstr`;复数条目与头部原样 | 翻译副本 |
| `renpy` | `.rpy` | Ren'Py `translate` 块:对白 + `old`/`new` 字符串 | 翻译副本 |
| `mtool` | `.json` | MTool `ManualTransFile.json`(内容嗅探) | 翻译副本 |
| `paratranz` | `.json` | Paratranz 导出;只填空的 `translation` | 翻译副本 |
| `vnt` | `.json` | VNTextPatch 导出(`name`/`message`) | 翻译副本 |
| `i18next` | `.json` | 叶子全为字符串的嵌套 JSON | 翻译副本 |
| `jsonl` | 文件/目录 | 万能逃生舱:任何引擎外部导出/写回 | `translated.jsonl` |

`attx formats` 输出机器可读清单。四种 `.json` 按内容嗅探区分,歧义时用 `--engine <id>` 强制。

暂不支持(欢迎贡献适配器,见[贡献指南](#贡献指南)):Translator++ 工程、XLSX、PDF、ASS 字幕、非 UTF-8 输入。

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
skills/attx/references/        # CLI 契约、开局方式、故障恢复、JSONL、反馈迭代
```

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
rpm = 60
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
| `doctor [--ping]` | 配置检查 / LLM 连通性 |
| `formats` | JSON 格式的适配器能力清单 |
| `detect --input <路径>` | 格式探测(保留 `--game` 别名) |
| `init --input <路径> --src --dst` | 建工作区 + SQLite |
| `extract --workspace` | 适配器 → 文本单元 |
| `translate --workspace [--limit] [--dry-run]` | 翻译 pending,批次增量落库 |
| `writeback --workspace [--dry-run]` | 产出译文文件 |
| `run --input …` | init + extract + translate + writeback |
| `status --workspace` | 进度统计 |
| `translate-jsonl` / `export-jsonl` / `import-jsonl` | 数据交换 |

全局:`--config /path/to/setting.toml`(默认 `./setting.toml` 或 `$ATTX_HOME/setting.toml`)。

---

## 贡献指南

欢迎 PR——尤其是新格式适配器。这个代码库刻意保持小而朴素,请维持这一点。

### 架构

```
src/
  main.rs          CLI (clap)
  model.rs         TextUnit / Translation / 控制符掩码 / 语言探测
  config.rs        setting.toml
  store.rs         SQLite 工作区(单元、译文、hash 缓存)
  llm.rs           OpenAI 兼容 chat、分批、并行 worker、按文体 profile 的 prompt
  quality.rs       行数 / 控制符完整性检查
  pipeline.rs      流程编排(不含格式知识;适配器不碰网络)
  adapter/
    mod.rs         FormatAdapter trait + 注册表 + 共享工具
    xmllite.rs     无损迷你 XML 树(epub/docx 共用)
    epub.rs docx.rs plaintext.rs subtitle.rs po.rs renpy.rs jsonkv.rs
    rmmz.rs rmmz_plugins.rs jsonl.rs
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

- Translator++(.trans)与 XLSX 适配器
- ASS 字幕适配器
- Shift-JIS / GBK 编码自动检测(encoding_rs)
- 跨批次术语表 / 译名固定
- PDF(经外部工具,同 AiNiee 借助 BabelDOC 的方式)

---

## 许可证

MIT
