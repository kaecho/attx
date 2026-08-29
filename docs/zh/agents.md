# Agent

attx 是一个本地 CLI，stdout 输出 JSON —— 已经是编程 agent 的工具面。**Skill** 就是执行协议：分阶段流水线、硬停止与 Q&A 配置向导。MCP 是可选的，通常没有必要。

## 安装 Skill

```bash
# Claude Code (personal, all sessions)
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
# project-scoped
mkdir -p .claude/skills && cp -a skills/attx .claude/skills/
```

其他 agent（Cursor / Codex / OpenCode / ……）：保留检出目录并要求：

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
```

文件：

```text
skills/attx/SKILL.md                                    # stages, hard stops, wizard
skills/attx/references/cli-command-contract.md          # exact CLI + JSON contract
skills/attx/references/agent-usage.md                   # session structure, pitfalls
skills/attx/references/custom-format-discovery.md       # unknown-format flow
skills/attx/references/failure-recovery.md              # symptom → action tables
skills/attx/references/jsonl-workflow.md                # JSONL interchange
skills/attx/references/feedback-iteration.md            # post-playtest feedback loop
```

## Q&A 配置向导

当配置缺失或 `doctor --ping` 失败时触发。agent 一次只问**一项**：

1. API 端点（OpenAI / DeepSeek / 以 `/v1` 结尾的自定义 `base_url`）
2. API Key → 只写入 `setting.toml`，绝不再打印，绝不进入聊天/日志/git
3. 模型名
4. 源语言 / 目标语言
5. 可选：并发、术语表开关（术语表默认关闭 —— 它有额外 LLM 费用）

然后：`attx doctor --ping` → 流水线第 0 阶段。

## 分阶段流水线

`detect` → `init` → `extract` → `status` → 试译 `translate --limit 20` → `learn note`（试译里有可复用习惯才写）→ 全量 `translate` → `review` → `writeback`（原地写回的格式先试运行）。每个阶段之后，汇报计数与下一步。

Skill 强制执行的硬停止：`doctor --ping` 401（不要反复重试刷 Key）、找不到适配器（切换到 Profile 工具链）、系统性试译失败（停止，别烧钱），以及**覆盖游戏目录的写回必须获得用户的明确许可**。

## 复制粘贴提示词

```text
Use the attx toolkit at <attx-dir>, following skills/attx/SKILL.md.

Help me set up attx if needed (Q&A: endpoint, key, model, languages),
then translate <input path> from Japanese into Simplified Chinese.

Rules:
1. Only the attx CLI; never hand-edit inputs, attx.db, or tool source.
2. Never print my API key.
3. doctor --ping → detect → init → extract → status → translate --limit 20 → full.
4. Prefer translated copies; ask before any in-place overwrite.
5. Report counts after each stage.
```

简短形式：

```text
Help me set up attx, then translate ./novel.epub from Japanese to Simplified Chinese.
```

## Agent 黄金法则

- **stdout 的 JSON 才是结果。** stderr 的 `batch i/n` 进度行不是。
- **绝不手工编辑**输入、`attx.db` 或工具源码 —— 一律通过 CLI 操作。
- **绝不回显 API Key** —— 它只存在于 `setting.toml`。
- agent 禁止使用 `learn review --approve-all`：`skip` 条目会删除文本，所以要带着证据汇报待批准条目，让用户决定。
- 收尾时 `status.passthrough > 0` 必须汇报；`--retry-passthrough` 会重新入队这些单元。
- 未知格式 → `analyze` → `profile new`/`test` → `init --profile`（绝不在 `detect` 失败时放弃）。
