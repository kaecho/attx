# 在 Agent 里如何跑 attx

面向：Cursor / Codex / Claude Code / OpenCode / 其他能执行命令的 Agent。

## 1. 准备（用户做一次）

1. 拿到 attx：克隆仓库或下载 Release 二进制  
2. 配置模型：

```bash
cd <attx目录>
cp setting.example.toml setting.toml
# 编辑 base_url / api_key / model
```

3. 本机验证：

```bash
attx doctor --ping
# 或
./target/release/attx doctor --ping
```

4. 在 Agent 中：
   - **打开游戏目录** 作为工作区根（推荐），或同时能访问 `<attx目录>` 与 `<游戏目录>`
   - 确保 Agent 能读到 `skills/attx/SKILL.md`（仓库内路径，或把 skills 拷进 Agent 的 skill 搜索路径）

## 2. 安装 Skill 的几种方式

### A. 仓库内 Skill（开发/源码用户）

路径：`<attx目录>/skills/attx/SKILL.md`  

在对话里明确：

```text
严格遵循 <attx目录>/skills/attx/SKILL.md
```

### B. 拷贝到 Agent 全局 skills 目录

按你使用的 Agent 文档放置，例如：

```bash
# 示例：某些工具使用 ~/.agents/skills 或项目 .agents/skills
mkdir -p <Agent的skills根>/attx
cp -a <attx目录>/skills/attx/* <Agent的skills根>/attx/
```

Skill frontmatter：

```yaml
name: attx
description: ...仅在用户明确要求执行 attx 翻译流程时使用...
```

由 Agent 的 skill 发现机制自动加载；**仅在用户要汉化时触发**。

### C. 发行包内附带（推荐给终端用户）

Release 资产中保留 `skills/attx/`，用户解压后提示词写：

```text
按 <发行包>/skills/attx/SKILL.md 执行
CLI：<发行包>/attx
配置：<发行包>/setting.toml
```

## 3. 推荐会话结构

| 角色 | 职责 |
|------|------|
| 主代理 | 跑 CLI、读 JSON、阶段推进、写回许可、对用户汇报 |
| 子代理（可选） | 只读抽查 JSONL 译文质量、汇总漏翻线索；**禁止 writeback / 改密钥** |

主代理每阶段结束输出四行：

1. 做了什么（命令）  
2. 关键数字（total/translated/pending 或 extracted）  
3. 风险/阻塞  
4. 下一步（或请用户决策点）

## 4. 标准命令序列（复制即用）

把 `<ATTX>`、`<GAME>`、`<WS>` 换成真路径：

```bash
ATTX=<attx目录>/target/release/attx   # 或 PATH 中的 attx
GAME=<游戏目录>
WS=<工作区>   # 常用 $GAME/.attx

$ATTX --config <attx目录>/setting.toml doctor --ping
$ATTX detect --game "$GAME"
$ATTX init --game "$GAME" --src ja --dst zh --workspace "$WS"
$ATTX extract --workspace "$WS"
$ATTX status --workspace "$WS"
$ATTX translate --workspace "$WS" --limit 20
$ATTX status --workspace "$WS"
# 用户确认后：
$ATTX translate --workspace "$WS"
$ATTX writeback --workspace "$WS" --dry-run
# 用户明确允许写回后：
$ATTX writeback --workspace "$WS"
```

**强烈建议**：先 `cp -a "$GAME" /tmp/game-copy` 再对副本写回。

## 5. 用户一句话触发示例

```text
用 attx（~/Desktop/workspace/Github/AT）按 skill 汉化
游戏：/path/to/Game/SM
源语言日文。先试译 20 条，全量前问我；写回前必须问我。
```

## 6. Agent 常见误区

| 误区 | 正确做法 |
|------|----------|
| 手改 Map001.json | `import-jsonl` + `writeback` |
| 把 Key 写进对话 | 只写在 setting.toml |
| 没 ping 就全量 | 先 `doctor --ping` 与 `--limit 20` |
| dry-run 当写回成功 | 看 `dry_run: false` 且 paths 落盘 |
| 与 att-mz 命令混用 | attx 无 `add-game`/`write-back` 连字符形式 |

## 7. 进度与费用控制

- `status.pending` 很大（如 >5000）：先 `--limit 50` 估时，问用户是否全量  
- 中断后直接再 `translate`：已译 hash 命中会跳过  
- 只要 pending 下降就继续；连续 2 轮 translated=0 且 pending>0 → 读 failure-recovery
