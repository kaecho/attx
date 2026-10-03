# attx

[English](README.md) | [完整文档](https://kaecho.github.io/attx/zh/) | [下载发行版](https://github.com/kaecho/attx/releases)

Agent Translation Toolkit eXtensible 是一个 Rust 命令行翻译工具，处理游戏、电子书、文档、字幕和本地化文件。它调用 OpenAI 兼容的 Chat Completions 接口，把进度保存在 SQLite 工作区中。

```text
识别格式 → 提取 → 可选术语表 → 翻译 → 检查与修复 → 安全写回
```

0.10.0 使用同一条管线支持普通 CLI 和 Agent。失败条目会在限定次数内定向重试；仍未解决的问题明确报告。用户要求正常翻译后，不再强制追加一次写回许可问答。

## 不想手工配置：复制给 Agent

打开[Agent 翻译向导](docs/zh/agent-translation.md)，复制完整提示词即可。Agent 自己阅读安装、使用和配置文档，询问 API、通过安全输入接收 Key，然后逐步解释模型、语向、费用、并发、术语表等选择，自动生成配置并开始翻译。你只回答问题，不需要手工编辑 TOML。

```text
请使用 https://github.com/kaecho/attx，先阅读项目安装、使用、配置文档和
skills/attx/SKILL.md、skills/attx/references/agent-setup.md。
我不想手工配置。请逐步询问翻译 API、通过安全输入或隐藏终端接收 Key，
然后询问模型、输入、语向、预算及参数。每一步说明这是什么、能做什么、
建议怎样配置以及费用/风险，由你安装工具、生成配置、校验并执行翻译。
已有可用配置就复用；不回显密钥，不把正常带备份写回设为另一轮许可问答。
```

## 安装

从 [GitHub Releases](https://github.com/kaecho/attx/releases) 下载对应平台的压缩包：Linux x86_64、Windows x86_64、macOS Apple Silicon 或 macOS Intel。发行包包含程序、配置示例、Agent Skill、自定义 Profile 示例和文档源码。

源码构建需要 Rust 1.89 或更新版本：

```bash
git clone https://github.com/kaecho/attx.git
cd attx
cargo build --release
./target/release/attx --help
# 可选：安装到 Cargo 的 bin 目录
cargo install --path . --locked
```

下文默认 `attx` 已加入 `PATH`。Windows 可以使用 `./attx.exe` 或完整程序路径。

## 第一次翻译

复制 `setting.example.toml` 为 `setting.toml`，在本机填写 `base_url`、`api_key` 和 `model`。不要把密钥发到 Agent 聊天或提交到 Git。

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
base_url = "https://your-provider.example/v1"
api_key = "YOUR_API_KEY"
model = "your-model-name"
```

接口必须支持 `{base_url}/chat/completions`。程序不直接实现 Anthropic、Gemini 或 Responses API；可以通过兼容网关接入。省略翻译、术语表和学习小节时，程序使用默认值。

```bash
attx --config ./setting.toml doctor --ping --json
attx --config ./setting.toml run --input "novel.epub" --src ja --dst zh
```

检查 `doctor` 输出中的 `llm.configured` 和 `ping`，不能只看退出码。`--ping` 会发起一个很小的模型请求，可能计费。

第二条命令输出 `novel.zh.epub`，不修改原书。工作区是输入旁的 `.attx-novel/`。目录输入一般使用 `<输入>/.attx/`。

PowerShell：

```powershell
Copy-Item setting.example.toml setting.toml
# 先在本机编辑 setting.toml，再执行以下命令
./attx.exe --config ./setting.toml doctor --ping --json
./attx.exe --config ./setting.toml run --input "C:/Books/novel.epub" --src ja --dst zh
```

只试译、不写输出：

```bash
attx run --input "novel.epub" --src ja --dst zh --limit 20 --no-writeback
attx translate --workspace ".attx-novel"
attx writeback --workspace ".attx-novel"
```

试译后还有待翻条目时，命令可能退出 2。已完成的批次仍在缓存中；再次执行 `translate` 会续跑。

## 支持格式与输出位置

| 输入 | 适配器 ID | 输出 |
|---|---|---|
| RPG Maker MV/MZ 游戏目录 | `rmmz` | 原地写 `data/*.json` 和 `js/plugins.js`，保留首次备份 |
| EPUB、HTML、Word、Excel | `epub`、`html`、`docx`、`xlsx` | 带目标语言后缀的同目录副本 |
| 纯文本、Markdown | `txt`、`md` | 带语言后缀的副本 |
| SRT、WebVTT、ASS/SSA、LRC | `srt`、`vtt`、`ass`、`lrc` | 带语言后缀的副本 |
| CSV/TSV、gettext PO/POT | `csv`、`po` | 带语言后缀的副本 |
| Ren'Py 翻译导出 | `renpy` | 带语言后缀的 `.rpy` 副本 |
| MTool、Paratranz、VNTextPatch、i18next JSON | `mtool`、`paratranz`、`vnt`、`i18next` | 带语言后缀的 JSON 副本 |
| JSONL 文本包 | `jsonl` | 文件副本，或目录下的 `translated.jsonl` |
| 按内容识别的结构化文本 | `auto` | 文件副本，或 `translated-<目标语言>/` 目录树 |
| 声明式 TOML Profile | `custom:<name>` | 默认副本；可显式配置原地写回 |

```bash
attx formats
attx detect --input "input.file"
attx analyze --input "input.file" --src ja
```

`auto` 可识别未知后缀中的 JSON 字符串、严格 XML 文本节点、INI/TOML/简单 YAML 的单行标量，以及能可靠判定的普通文章。它保护未改动的结构字节；译文可在原编码中表示时，保留原编码和 BOM。

Auto 目录模式会把不支持的文件原样复制，并在 `extract.auto_coverage` 中报告。它不会在目录树中自动调用全部文档、压缩包、字幕或脚本适配器。复制文件不等于翻译文件。二进制封包、加密资产、含糊脚本和不支持的语法仍需要专用适配器或外部提取器。

普通探测失败时，`run` 可以让已配置的模型生成声明式 Profile。程序校验提取结果和保持原文的空操作 roundtrip，强制 `overwrite = false`，最多接受三轮提案。`--no-infer` 关闭这项额外计费操作。模型仍可能选择不完整或语义不对的字段，roundtrip 不能证明覆盖完整。

```bash
attx profile infer --input "scene.scn" --output ./scene.toml --src ja --name scene
attx profile test --profile ./scene.toml --input "scene.scn" --roundtrip
attx run --input "scene.scn" --profile ./scene.toml --src ja --dst zh
attx profile save --profile ./scene.toml
```

详见[格式能力与限制](docs/zh/formats.md)和[自定义 Profile](docs/zh/profiles.md)。

## 自动检查与修复

模型结果入库前，程序检查编号、行结构和保护标记的精确数量，拒绝被截断的响应。缺失或不合格条目缩小批次后重试，已经通过的同批条目不重复翻译。

中文目标语默认做保守的机械规范化：上下文能确认是中文时，删除独立的 `っ`/`ッ`，把 `っすよ`/`っす` 改为 `哦`，把 `ー` 改为 `～`。整句日文、引用的字形说明和保护片段不会被机械删掉，`・` 仍保留。

RPG Maker 消息重排保护第 0 槽中的独立 `【名字】`，正文控制符保持完整，不增删事件命令。两个槽只能装一个姓名和一整句台词时，正文可能超过默认 44 个半宽单元的估计宽度；程序报告超宽，不会为了压行把姓名粘进台词。

```bash
attx review --workspace ".attx-novel"
attx repair --workspace ".attx-novel"
attx writeback --workspace ".attx-novel" --dry-run
```

审校报告包含源语残留、`kana_edge`、`kana_mixed`、`kana_untranslated`、原文照抄、保护码丢失、姓名栏不一致，以及术语表建议。报告样本有上限，实际修复候选不受样本上限限制。成功写回会把规范化和重排后的行同步到缓存，后续导出能看到实际渲染行。

任何模型都不能保证任意格式一次翻译完美。检查可以发现特定的结构和文字系统问题，但不能证明语义、人物口吻、提取完整性或真实游戏画面都正确。[质量与修复](docs/zh/quality.md)说明了这些边界。

## 配置与费用

配置查找顺序：`--config`，然后是存在的 `$ATTX_HOME/setting.toml`，最后是当前目录的 `setting.toml`。`--client <name>` 临时选择另一个客户端。

```toml
[translation]
worker_count = 8
rpm = 60
retry_count = 3
retry_delay = 2
batch_chars = 2500
max_context_items = 6
repair_rounds = 2
context_chars = 1200

[glossary]
enabled = false
min_occurrences = 10
max_terms = 200
inject_limit = 30

[learn]
auto_summarize = true
llm_review = false
```

每个被选中的单元，每轮最多进入 `1 + retry_count` 次请求。`translate` 和 `run` 在初始轮之后，最多再对未解决的选中条目执行 `repair_rounds` 轮修复。设为 0 可关闭额外审校轮；请求级重试仍受 `retry_count` 限制。`context_chars` 是每个请求的前后文总字符预算，不是模型上下文窗口。

术语表自动构建默认关闭，会额外调用模型。已有生效术语仍按批次注入。经验总结默认在本地执行，只有开启 `llm_review` 才增加模型复核。会删除提取条目的学习规则仍需按编号批准，Agent 不得批量批准。

[配置参考](docs/zh/configuration.md)逐项说明所有参数、类型、默认值、可选模型字段、`extra` 覆盖顺序、环境变量和调优方法。[工作流程](docs/zh/usage.md)给出术语表、保护规则与学习命令的用法。

## 写回安全与状态

正常翻译请求包含其常规输出路径，不再强制追加写回确认。程序在执行时检查风险：

- 默认存在待翻或不合格条目就阻止写回。显式 `--allow-partial` 只应用有效译文，让未解决条目保留原文。
- 首次覆盖已有文件前，生成 `{原路径}.attxbak`，以后不覆盖这份备份。备份失败则停止写入。
- 所有输出先暂存，再逐文件替换。单文件替换是原子的，整个目录不是一个文件系统事务。
- 工作区修改使用操作系统锁；工作区绑定输入、引擎、Profile 快照和语向。
- 源单元或锚点变化后，必须重新提取。RPG Maker 优先使用原始快照或首次备份，避免把已写回的译文当作新原文。
- dry-run 不修改缓存中的规范化结果，也不替换输出文件。

| 退出码 | 含义 |
|---|---|
| `0` | 命令完成，或返回 dry-run 计划 |
| `1` | 执行、配置、源文完整性或 HTTP 错误 |
| `2` | 翻译未完成、写回被阻止，或显式部分输出仍需处理 |

退出 2 时，stdout 仍输出 JSON，包含 `status`、数量和审校详情。`doctor` 与建议性质的 `review` 有自己的报告语义，不要仅凭退出码推断翻译已经完成。

## 给 Agent 使用

执行协议是 [`skills/attx/SKILL.md`](skills/attx/SKILL.md)，配套参考文档说明问答配置、CLI 字段、恢复和未知格式。首次配置可用[解释式向导](docs/zh/agent-translation.md)；已有有效配置不重复提问，正常写回不再追加许可。

可以直接发给 Agent：

```text
按照 <attx目录>/skills/attx/SKILL.md，使用本机 attx CLI。
把 <输入路径> 从日文翻译成简体中文。
执行常规管线，在配置次数内修复检测到的问题，然后带备份写回。
报告输出路径和未解决问题，不打印密钥，不手改输入或数据库。
```

Claude Code 的本地安装示例：

```bash
mkdir -p ~/.claude/skills
cp -a skills/attx ~/.claude/skills/
```

其他 Agent 可直接读取仓库 Skill，不需要 MCP 服务或内置 Agent 运行时。无关删除、重置工作区，以及超出本次任务的付费范围，仍应单独取得授权。

## 文档与二次开发

完整手册提供[中文](docs/zh/index.md)、[English](docs/en/index.md)和[日本語](docs/ja/index.md)。从[快速开始](docs/zh/quickstart.md)进入，再按需要查阅 [CLI](docs/zh/cli.md)、[工作区与 JSONL](docs/zh/workspace.md)、[架构](docs/zh/architecture.md)和[二次开发](docs/zh/development.md)。

```bash
cargo test -- --test-threads=1
cargo clippy --all-targets
cargo build --release
pip install -r requirements-docs.txt
mkdocs build --strict
```

Rust 测试全部放在对应源码文件中。适配器只负责提取和生成输出，管线负责网络、缓存和文件提交。新增适配器、Profile 规则、配置字段或 CLI 命令前，先看开发指南。

本轮研究了 [LinguaGacha](https://github.com/neavo/LinguaGacha) 的定向重试、Agent 与批量翻译共用服务、有界上下文、严格结构检查和文本保护，使用 Rust 独立实现，没有复制其源码。它的商业使用声明不构成对 attx 的许可证授权。格式适配器设计也参考了 [AiNiee](https://github.com/NEKOparapa/AiNiee)。

变更记录：[CHANGELOG.md](CHANGELOG.md)。attx 采用 [MIT 许可证](LICENSE)。
