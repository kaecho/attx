---
name: attx
description: >
  通用 AI 翻译执行协议。当用户要求翻译游戏（RPG Maker MV/MZ、MTool、Ren'Py、
  VNText）、电子书（EPUB）、网页（HTML）、文档（DOCX/XLSX/TXT/Markdown）、字幕
  （SRT/VTT/ASS/LRC）、表格（CSV/TSV）或本地化文件（PO/Paratranz/i18next/JSONL）
  时使用：探测格式、问答式配置、提取文本、LLM 翻译、写回输出、试玩/审阅反馈补漏。
  遇到不支持的格式时：analyze 侦察 → 写自定义 Profile → 试跑 → 翻译 → 保存记住格式。
---

# attx Skill

本 Skill 是 **翻译任务执行协议**，不是项目说明书。  
主文件只做路由：触发边界、配置向导、阶段索引、硬停止。细节读 `references/`。

仓库：https://github.com/kaecho/attx  
人类文档：`README.md`（英文） / `README.zh-CN.md`（中文）

---

## 支持的格式（v0.4+）

运行 `attx formats` 获取机器可读清单（JSON，含已保存的自定义 Profile）。当前适配器：

| id | 输入 | 说明 | 输出 |
|----|------|------|------|
| `rmmz` | 目录 | RPG Maker MV/MZ（data/*.json + plugins.js 参数） | 原地写回 + `*.attxbak` |
| `epub` | .epub | 电子书/轻小说（段落级，保留插图与排版，rt 注音剔除） | `<名>.<目标语言>.epub` |
| `html` | .html/.htm/.xhtml | 单页 HTML（块级 + `<title>`） | 同名副本 |
| `docx` | .docx | Word 文档 | `<名>.<目标语言>.docx` |
| `xlsx` | .xlsx/.xlsm | Excel（译 sharedStrings，全表一致生效） | 同名副本 |
| `txt` / `md` | 文件 | 小说纯文本 / Markdown（跳过代码块，保留语法前缀） | 同名 `.zh.txt` 等 |
| `srt` / `vtt` / `lrc` | 文件 | 字幕/歌词（时间轴原样保留） | 同名副本 |
| `ass` | .ass/.ssa | ASS/SSA 字幕（`{\tag}` 与 `\N` 保留，Name 作角色） | 同名副本 |
| `csv` | .csv/.tsv | 表格（RFC4180，含引号/内嵌换行；只重写有译文的记录） | 同名副本 |
| `po` | .po/.pot | Gettext（填 msgstr；复数条目跳过） | 同名副本 |
| `renpy` | .rpy | Ren'Py `translate` 块（对白 + old/new strings） | 同名副本 |
| `mtool` | .json | MTool ManualTransFile（内容嗅探） | 同名副本 |
| `paratranz` | .json | Paratranz 导出（只译空 translation） | 同名副本 |
| `vnt` | .json | VNTextPatch 导出 | 同名副本 |
| `i18next` | .json | 嵌套字符串 JSON（≥80% 字符串叶子） | 同名副本 |
| `jsonl` | 文件/目录 | 通用逃生舱：任何引擎外部导出 | `translated.jsonl` |
| `custom:<名>` | 文件/目录 | **自定义 Profile**（TOML 规则，agent 可自行编写并保存） | 副本或原地（看 Profile） |

`.json` 由内容嗅探区分四种；歧义时 `--engine <id>` 强制。  
文本类输入编码自动检测（UTF-8 / Shift-JIS / GBK / UTF-16 BOM），输出一律 UTF-8。  
不支持（roadmap）：Translator++ 工程、PDF、二进制封包（走 JSONL 逃生舱）。

---

## 核心原则

1. **只通过 attx CLI、工作区 SQLite、JSONL、输入文件与用户明确信息流转业务数据。**
2. **禁止手改**输入文件、`attx.db`、工具源码；一律走 CLI。
3. **密钥只存在 `setting.toml`**；禁止写入任务单、报告、聊天记录或 git 提交。
4. **主代理是总控**：写回许可（rmmz 原地写回时）、全量重译、改源码，必须主代理裁决。
5. **默认自动推进**：能靠 CLI 输出判断的下一步自行执行并报告；只在真实决策点问用户。
6. **stdout 最终 JSON 才是结果**；stderr 的 `batch …` 进度行不是最终结果。
7. 文档类格式输出**翻译副本**（原文件永不修改）；只有 `rmmz` 会原地写回（有 `*.attxbak`）。

---

## 运行面（路径约定）

| 占位符 | 含义 |
|--------|------|
| `<attx目录>` | attx 源码或安装位置（含 `attx` 二进制 / `cargo run`） |
| `<输入>` | 目标文件（epub/docx/srt/…）或游戏根目录 |
| `<工作区>` | 目录输入默认 `<输入>/.attx`；文件输入默认 `<父目录>/.attx-<文件名>` |
| `<配置>` | `setting.toml`：默认 `<attx目录>/setting.toml`，或 `--config` / `$ATTX_HOME/setting.toml` |

### CLI 入口选择（按优先级）

```text
1) 已安装：attx <子命令> ...
2) 发行包：<attx目录>/attx <子命令> ...
3) 已编译：<attx目录>/target/release/attx <子命令> ...
4) 源码：  cd <attx目录> && cargo build --release 后用 3)
```

Agent 启动时必须先解析出本机真实入口，并在任务单写死。  
兼容提示：`--input` 与旧参数 `--game` 等价。

---

## 阶段 -1 — 配置向导（问答式，首次/配置失效时强制）

触发条件：`<配置>` 不存在，或 `attx doctor` 报 "llm: not configured"，或 `doctor --ping` 失败。

**用问答收集配置**（有 AskUserQuestion 类工具时优先用它；没有就在对话里逐项问，一次一项）：

1. **API 端点**：问用户使用哪个 OpenAI 兼容服务。给选项：
   - OpenAI 官方（`https://api.openai.com/v1`）
   - DeepSeek（`https://api.deepseek.com/v1`）
   - 本地/中转（让用户给出完整 `base_url`，通常以 `/v1` 结尾）
2. **API Key**：请用户把 Key 粘贴为**单独一条消息**或自行写入 `setting.toml`。
   Agent 拿到后**立即写入 `setting.toml`，此后任何输出不得回显**（日志、总结、提交一律禁止）。
3. **模型名**：用户指定（如 `gpt-4.1-mini`、`deepseek-chat` 或中转站模型名）。
4. **语向**：源语言 `ja|en`，目标语言（默认 `zh`；支持 `zh-tw`/`en`/`ko` 等）。
5. **并发与预算**（可选，给默认值让用户确认）：`worker_count=4`、`batch_chars=2500`。
6. **术语表**（默认关闭，必须主动问）：
   > 是否开启自动术语表？开启后 attx 会在翻译前额外调用模型统一专有名词译名，
   > **显著提升长篇作品的一致性**。默认 `method=llm`（模型读原文抽术语，费用跟
   > 文本批次数走）；也可 `method=stats`（正则挖高频词再命名，费用跟术语数走，
   > 更便宜）。调高 `max_terms` 可多收，费用与噪音同时上升。

   用户同意 → `[glossary] enabled = true`；不确定 → 保持 `false`，并告知随时可用
   `attx glossary build` 单独构建（该命令不受开关限制；可用 `--method`）。

然后生成 `<attx目录>/setting.toml`（模板见 `references/cli-command-contract.md`），执行：

```bash
attx doctor --ping
```

- 成功 → 汇报"配置完成，模型 X 可用"，进入阶段 0。
- 401/Invalid token → **硬停止**，请用户检查 Key；禁止重试刷 Key、禁止猜 Key。

---

## 阶段索引

| 阶段 | 目标 | 命令 | 通过标准 |
|------|------|------|----------|
| 0 启动 | CLI 可用、配置可用 | `doctor` / `doctor --ping` | 无配置错误 |
| 1 探测 | 识别格式 | `detect --input <输入>` | 返回 `engine` 与 `content_root` |
| 2 初始化 | 建工作区 | `init --input ... --src ja\|en --dst zh [--workspace]` | 返回 `workspace` |
| 3 提取 | 入库文本单元 | `extract --workspace <工作区>` | `extracted > 0` |
| 4 状态 | 进度事实 | `status --workspace` | 记录 total / translated / pending |
| 4.5 术语表 | 统一专有名词（仅在用户开启时） | `glossary build --workspace`（先 `--dry-run` 报候选数与费用） | `total_active > 0` |
| 5 试译 | 小批量验模型 | `translate --limit 20` | 有成功条目；无规则性全败 |
| 6 全量译 | 清 pending | `translate` 多轮 | pending 下降；可 `export-jsonl` 审校 |
| 7 写回 | 产出译文文件 | `writeback`（rmmz 先 `--dry-run` 并取得许可） | files>0；文档类产出 `<名>.<语言>.<扩展名>` |
| 7.5 回检 | 机械审校（无 LLM） | `review --workspace` | 向用户报告 residual_source / identical / control_loss / namebox_mismatch / glossary.violations；有命中则 export-jsonl 修 |
| 7.6 沉淀 | 把本轮**可复用的翻译习惯**写成 prompt note | `learn note --workspace --name <短名> --text "…"` | 1–5 条具体指令；空话不写 |
| 8 反馈 | 补漏 | export/import/translate/writeback | 问题可定位并再写回 |

`writeback` 成功后 attx 会**自动**把本轮**提取**经验写入知识库（零 API 成本）：哪些字段不该译。这学不到文风。若输出里
`writeback.learned.pending > 0`，说明有会**删除文本**的规则待批准——向用户报告条数，
让其用 `attx learn pending` / `attx learn review --approve <n>` 裁决，**agent 不得自行
`--approve-all`**。

翻译腔调、敬称、人称、namebox 习惯不在数据库的统计里。agent 用 `attx learn note` 写
`topic=prompt` 的 note，下一轮 `translate` 会注入系统提示词。已经译完的条目不会重跑；要对齐文风，只译还 pending 的，或经用户同意后重译。专有名词走 `glossary`，不走 note。

一条龙（用户同意整包自动时）：

```bash
attx run --input <输入> --src ja --dst zh [--limit N]
```

### 阶段 1b — 未知格式（detect 失败时）

不停止、不放弃：走 `references/custom-format-discovery.md` 完整流程：

```bash
attx analyze --input <输入>                 # 侦察：编码/结构/样本
attx profile new --output ./fmt.toml        # 起草规则（line_regex / json_keys / json_paths）
attx profile test --profile ./fmt.toml --input <输入> --roundtrip   # 迭代到 units/样本正确
attx init --input <输入> --profile ./fmt.toml --src ja --dst zh     # 后续流程照常
```

翻译成功后**问用户是否 `attx profile save` 记住该格式**（下次自动识别）。
纯二进制/加密封包 → JSONL 逃生舱（`references/jsonl-workflow.md`）。

### 关键差异：写回许可

- **文档/字幕/JSON 类**（epub/docx/txt/md/srt/vtt/lrc/po/renpy/mtool/paratranz/vnt/i18next/jsonl）：
  输出是**新副本**，不碰原文件 → **无需写回许可**，翻完直接 writeback 并报告输出路径。
- **rmmz（原地写回游戏目录）**：必须先 `--dry-run`，取得用户明确许可后再真写。

### 提取后必须报告规模

`status` 的 pending 很大时（>2000），向用户报告条数与费用/时长风险，问是否全量或先部分。


### 阶段 5 之后：先沉淀再全量

试译里若看出可复用的习惯（敬称保留、女主软口语、namebox 与对白人称一致），**先写 note 再全量**：

```bash
attx learn note --workspace <工作区> --name honorifics --text "角色名后的さん/くん/ちゃん保留不译"
attx learn note --workspace <工作区> --name voice --text "女主用软口语，反派短句、少语气词"
```

- 一条一个事实，总共 1–5 条。禁止空话（「保持一致」「流畅自然」）。
- 本作品 → `--workspace`（写入 `<工作区>/experience.toml`）。该格式以后都这样 → `--format rmmz`（不要 workspace）。
- `--name` 是 upsert 键，同名覆盖，异名并存。
- 禁止手改 `experience.toml`。
- **不要**指望 `learn summarize --llm` 写出文风：它只复核「这个字段是不是标识符」。
- 写完用 `attx learn list --workspace <工作区>` 确认；下一轮 `translate` 的 stderr 会出现 `applying N learned prompt note(s)`。

没有具体观察就跳过，不要为写而写。

---

## 硬停止

| 条件 | 动作 |
|------|------|
| CLI 找不到 / 无法启动 | 停止，说明如何安装或 `cargo build --release` |
| `doctor --ping` 401 / 无效 Key | 停止，走阶段 -1 配置向导；禁止重试刷 Key |
| `detect` 无适配器 | 转阶段 1b：analyze → 自定义 Profile；纯二进制转 JSONL 流程 |
| 提取 0 且输入明显有文本 | 停止，报告格式/路径问题（可能需 `--engine` 强制） |
| 试译连续失败（格式/质量） | 停止全量，先排障（`references/failure-recovery.md`） |
| rmmz / `overwrite=true` Profile 未获写回许可 | **禁止** `writeback`（dry-run 除外） |
| `status.passthrough > 0` 收尾时 | 报告条数；经用户同意后 `translate --retry-passthrough` |
| `writeback.learned.pending > 0` | 报告条数并交用户裁决；**禁止** agent 自行 `learn review --approve-all` |
| 术语表 `build` 前 pending 很大 | 先 `--dry-run` 报候选数与预计费用，取得同意再真建 |
| 用户要求改 attx 源码 | 暂停翻译流程，单独走源码任务 |
| 用户要求重置/删库 | 需明确确认后再删 `<工作区>` |

---

## 禁止做法

- 手改输入文件 / `attx.db` / `experience.toml` 冒充翻译完成或经验沉淀（note 走 `learn note`）
- 把 API Key 写进 prompt、JSONL、日志、git、总结
- 未试译直接对上万条全量硬刚且不告知费用风险
- 把 dry-run 成功说成已经写出文件
- 修改 attx 源码"顺便修 bug"而不经用户同意
- 写空话 prompt note（「保持一致」「流畅自然」）污染下一轮翻译

---

## 按需参考

| 工作 | 必读 | 何时读 |
|------|------|--------|
| 命令与 JSON 字段 | `references/cli-command-contract.md` | 任何 CLI 调用前 / 解析输出时 |
| Agent 如何开局 | `references/agent-usage.md` | 新会话第一次接到翻译任务时 |
| **未知格式 → 自定义 Profile** | `references/custom-format-discovery.md` | `detect` 失败、用户要求"记住格式"时 |
| 失败与重试 | `references/failure-recovery.md` | 401/超时/质量失败/写回失败时 |
| 二进制封包 / 通用 JSONL | `references/jsonl-workflow.md` | Profile 也搞不定，或用户要离线审校时 |
| 试玩/审阅反馈 | `references/feedback-iteration.md` | 用户反馈漏翻/误翻/显示问题 |

不要把整份 reference 塞进模型 prompt；只读当前阶段需要的小节。

---

## 给用户的开场提示词（可复制）

```text
请使用 attx 工具包（目录：<attx目录>）按 skills/attx/SKILL.md 流程，
把 <输入文件或游戏目录> 从日文翻译为简体中文。

约束：
1. 只通过 attx CLI 操作；禁止手改输入文件、attx.db、工具源码。
2. 若未配置模型，先用问答向导帮我配置 setting.toml；不要把 API Key 打进对话记录。
3. 先 doctor --ping、detect、init、extract、status；再 limit 20 试译；有可复用习惯则 learn note，再全量 translate；写回前跑 review。
4. RPG Maker 游戏写回前必须得到我明确允许；文档类直接产出翻译副本即可。
5. 每阶段结束用中文汇报：做了什么、status 数字、下一步、是否需要我决策。
```
