---
name: attx
description: >
  仅在用户明确要求执行或继续 attx 游戏文本翻译流程时使用：探测引擎、初始化工作区、
  提取文本、调用模型翻译、JSONL 导入导出、写回游戏文件、试玩反馈补漏。
  支持 RPG Maker MV/MZ（rmmz）与通用 JSONL 管线。
---

# attx Skill

本 Skill 是 **翻译任务执行协议**，不是项目说明书。  
主文件只做路由：触发边界、运行面、阶段索引、硬停止。细节读 `references/`。

仓库：https://github.com/emptysuns/attx  
人类文档：`README.md`（英文） / `README.zh-CN.md`（中文）

---

## 核心原则

1. **只通过 attx CLI、工作区 SQLite、JSONL、游戏目录与用户明确信息流转业务数据。**
2. **禁止手改**游戏 `data/*.json`、插件源码、`attx.db`、工作区缓存；一律走 CLI。
3. **密钥与模型只存在 `setting.toml`**；禁止写入任务单、报告、聊天记录或 git 提交。
4. **主代理是总控**：写回许可、全量重译、改源码/Skill、接受不可逆风险，必须主代理裁决；不得交给子代理擅自执行。
5. **默认自动推进**：能靠 CLI 输出判断的下一步，自行执行并报告；只在真实决策点停下来问用户。
6. **stdout 最终 JSON 才是结果**；stderr 的 `batch …` 进度行不是最终结果。
7. **第一次写回 = 可试玩初版**，不是 100% 完成；稳定版依赖用户试玩反馈再补。

---

## 运行面（路径约定）

| 占位符 | 含义 |
|--------|------|
| `<attx目录>` | attx 源码或安装位置（含 `attx` 二进制 / `cargo run`） |
| `<游戏目录>` | 目标游戏根目录（RM 应含 `data/`、`js/` 等） |
| `<工作区>` | 默认 `<游戏目录>/.attx`，或用户指定的独立目录 |
| `<配置>` | `setting.toml`：默认 `<attx目录>/setting.toml`，或 `--config` / `$ATTX_HOME/setting.toml` |

### CLI 入口选择（按优先级）

```text
1) 已安装：attx <子命令> ...
2) 发行包：<attx目录>/attx <子命令> ...
3) 源码：  cd <attx目录> && cargo run --release -- <子命令> ...
4) 已编译：<attx目录>/target/release/attx <子命令> ...
```

下文统一写 `attx ...`；Agent 启动时必须先解析出本机真实入口，并在任务单写死。

### 只读边界

- 翻译流程中 **`<attx目录>` 源码默认只读**。未经用户明确允许，禁止改源码、Skill、配置模板、CI。
- 写回会改 **`<游戏目录>` 下数据文件**；写前必须取得用户许可。写回会生成 `*.attxbak` 单次备份。

---

## 按需参考

| 工作 | 必读 | 何时读 |
|------|------|--------|
| 命令与 JSON 字段 | `references/cli-command-contract.md` | 任何 CLI 调用前 / 解析输出时 |
| Agent 如何开局 | `references/agent-usage.md` | 新会话第一次接到汉化任务时 |
| 失败与重试 | `references/failure-recovery.md` | 401/超时/质量失败/写回失败时 |
| 非 RM / 通用 JSONL | `references/jsonl-workflow.md` | 引擎非 rmmz，或用户要离线审校时 |
| 试玩反馈 | `references/feedback-iteration.md` | 用户反馈漏翻/误翻/显示问题 |

不要把整份 reference 塞进模型 prompt；只读当前阶段需要的小节。

---

## 阶段索引

| 阶段 | 目标 | 命令 | 通过标准 |
|------|------|------|----------|
| 0 启动 | CLI 可用、配置可用 | `doctor` / `doctor --ping` | 无配置错误；本轮要翻译则 ping 成功 |
| 1 探测 | 识别引擎 | `detect --game <游戏目录>` | 返回 `engine`（如 `rmmz`）与 `content_root` |
| 2 初始化 | 建工作区 | `init --game ... --src ja\|en --dst zh [--workspace]` | 返回 `workspace` 路径 |
| 3 提取 | 入库文本单元 | `extract --workspace <工作区>` | `extracted > 0`（或游戏确实无文本） |
| 4 状态 | 进度事实 | `status --workspace` | 记录 total / translated / pending |
| 5 试译 | 小批量验模型 | `translate --limit 20` | 有成功条目；无规则性全败 |
| 6 全量译 | 清 pending | `translate` 多轮 | pending 下降；可 `export-jsonl` 审校 |
| 7 写回 | 进游戏文件 | `writeback`（先 `--dry-run`） | 用户已许可；files>0 或无可写译文 |
| 8 反馈 | 补漏 | export/import/translate/writeback | 问题可定位并再写回 |

一条龙（仅在用户同意整包自动时）：

```bash
attx run --game <游戏目录> --src ja --dst zh [--workspace <工作区>] [--limit N]
```

---

## 新游戏主流程（主代理必须按序）

### 阶段 0 — 启动

1. 解析 `attx` 入口；运行 `attx doctor`。
2. 若本轮需要模型：确认 `<配置>` 存在且 `attx doctor --ping` 成功。  
   - 401/Invalid token：**硬停止**，让用户修 Key，禁止猜 Key。
3. 向用户确认：`<游戏目录>`、源语言 `ja|en`、是否允许写回（可后置到阶段 7）。

### 阶段 1–3 — 探测 / 初始化 / 提取

```bash
attx detect --game <游戏目录>
attx init --game <游戏目录> --src <ja|en> --dst zh --workspace <工作区>
attx extract --workspace <工作区>
attx status --workspace <工作区>
```

- `detect.engine` 必须记录。非 `rmmz` 且无适配器 → 转 `references/jsonl-workflow.md`。
- `rmmz` 提取会包含 **`js/plugins.js` 插件参数**（读 `plugins/*.js` 头部 `@param/@type` 判断用户可见字段；**禁止**改插件源码，写回只动 `plugins.js`）。
- 大游戏提取后用 `status` 向用户报告条数与预估费用风险（pending 很大时建议先 `--limit` 试译）。

### 阶段 5 — 小批量试译（强制）

```bash
attx translate --workspace <工作区> --limit 20
attx status --workspace <工作区>
```

- 目的：验证 API、模型 JSON 格式、控制符是否被破坏。
- **禁止**为了“小批量 0 失败”而手改 db 或跳过失败条目伪造成功。
- 试译成功再进入全量。

### 阶段 6 — 全量翻译

```bash
attx translate --workspace <工作区>
# pending 仍高：再跑，依赖缓存跳过已译
attx status --workspace <工作区>
```

可选人工审校：

```bash
attx export-jsonl --workspace <工作区> --output <工作区>/pending.jsonl --filter pending
# 用户/外部工具改 translation_lines 后：
attx import-jsonl --workspace <工作区> --input <工作区>/translated.jsonl
```

### 阶段 7 — 写回（硬门槛）

**全部满足才执行真正 writeback：**

1. 用户 **明确允许写回** 到该 `<游戏目录>`（建议先在副本试写）。
2. `status` 显示关键路径已有足够译文（或用户接受部分写回）。
3. 先：

```bash
attx writeback --workspace <工作区> --dry-run
```

4. 再：

```bash
attx writeback --workspace <工作区>
```

5. 报告改动的 `paths`，提醒用户启动游戏试玩；说明存在 `*.attxbak`。

### 阶段 8 — 试玩反馈

见 `references/feedback-iteration.md`。原则：定位 → 补译/import → 再 writeback；禁止手改 data。

---

## 给用户的开场提示词（可复制）

Agent 打开 **游戏目录** 后，用户可发送（替换尖括号）：

```text
请使用 attx 工具包（目录：<attx目录>）按 skills/attx/SKILL.md 流程，
对当前游戏做日文→简体中文汉化。

约束：
1. 只通过 attx CLI 操作；禁止手改游戏 data、attx.db、工具源码。
2. 模型配置只用 <attx目录>/setting.toml；不要把 API Key 打进对话或文件。
3. 先 doctor --ping、detect、init、extract、status；再 limit 20 试译；通过后全量 translate。
4. writeback 前必须得到我明确允许；优先在游戏副本上写回。
5. 每阶段结束用中文汇报：做了什么、status 数字、下一步、是否需要我决策。
```

发行包场景把入口改成发行包内二进制路径即可。

---

## 硬停止

| 条件 | 动作 |
|------|------|
| CLI 找不到 / 无法启动 | 停止，说明如何安装或 `cargo build --release` |
| `doctor --ping` 401 / 无效 Key | 停止，请用户修 `setting.toml`，禁止重试刷 Key |
| `detect` 无适配器 | 停止或转 JSONL 流程，不得伪造 rmmz |
| 提取 0 且游戏明显有文本 | 停止，报告引擎/路径问题 |
| 试译连续失败（格式/质量） | 停止全量，先排障（见 failure-recovery） |
| 未获写回许可 | **禁止** `writeback`（无 dry-run 以外） |
| 用户要求改 attx 源码 | 暂停翻译流程，单独走源码任务 |
| 用户要求重置/删库 | 需明确确认后再删 `<工作区>` |

---

## 禁止做法

- 手改 `data/*.json` / 插件 / `attx.db` 冒充翻译完成  
- 把 API Key 写进 prompt、JSONL、日志、git  
- 子代理执行 `writeback` / 删除工作区 / 改 setting.toml 密钥  
- 未试译直接对上万条全量硬刚且不告知费用风险  
- 把 dry-run 成功说成已经写进游戏  
- 把初版写回宣传成“全部完成”  
- 修改 attx 源码“顺便修 bug”而不经用户同意  

---

## 与 att-mz 的差异（Agent 勿混用）

| | att-mz | attx |
|--|--------|------|
| 实现 | Python + Rust 扩展 | 纯 Rust 单二进制 |
| 范围 | RM 深度（规则 Agent、插件 AST、术语工程…） | 通用核心 + rmmz/jsonl |
| 工作区 | 工具 data/db + agent workspace JSON | 游戏旁 `.attx` SQLite |
| 命令 | `add-game` / `write-back` / 60+ | `init` / `writeback` / 十余条 |
| Skill | 多阶段规则审查 | 本 Skill：精简 extract→translate→writeback |

用户点名 att-mz 时用 att-mz Skill；点名 attx / 通用框架 / JSONL 管线时用本 Skill。
