# 自我改进的提取知识层（Knowledge Layer）

日期：2026-07-28
状态：已批准，待实现

## 背景

attx 的提取阶段依赖硬编码启发式判断「哪些字符串是玩家可见文本」。以 `rmmz_plugins.rs` 为例：`visible_key()` 白名单 + `skip_key()` 黑名单 + `is_machine_literal()` 值检查。

这些表是人写死的，因此会错：

- 2026-07-28 修复的 bug 中，`skip_key` 有 `id`/`symbol` 却漏了 `key`，导致成就身份句柄 `実績_xxx` 被送去翻译。事件脚本按原文引用它（`gainAchievement("実績_xxx")`），翻译后成就永远无法解锁。
- 反向的漏洞同样存在：某些含日文的可见字段因不在白名单、又不含足够 CJK 特征而被跳过，造成漏译。

每次踩到这类坑，修复只留在源码里，**下一个游戏、下一个用户重新踩一遍**。

## 目标

让 attx 把「某格式的某字段该不该提取」这一类判断，作为可积累、可复用、跨项目的知识沉淀下来。

明确的非目标：
- 不学习翻译风格 / 术语一致性（翻译阶段已有 retry → split → passthrough 与 `quality::check_unit` 兜底，且风格判断主观）
- 不构建规则 DSL（正则、值条件、布尔组合一律不做）
- 不自动改变行为（默认必须人工批准）

## 为什么选提取阶段

翻译阶段的错误可自愈：重试、拆批、passthrough、质量校验层层兜底。

提取阶段的错误**没有安全网**。漏提的文本永远不会被翻译；误提的机器字段会一路写回，直到游戏运行时才炸。今天修复的两个缺陷都发生在这一层。

而且提取判断是**二元且客观可查**的：一个字段要么是身份句柄要么是玩家可见文本。这是可学习的分类问题，不像「译文风格好不好」那样主观。

## 架构

```
提取时                                   学习时                      审核时
─────────                                ──────                      ──────
pipeline::extract                        attx learn scan             attx learn review
  │                                        │                           │
  ├─ adapter.extract()  ← 不改             ├─ 读 workspace DB          ├─ 列出 pending 提案
  │                                        │  （客观信号）              ├─ approve / reject
  └─ knowledge::apply() ← 新增             └─ 可选 LLM 复盘 →          └─ 写入 <format>.toml
       ↓                                        proposals.toml
     过滤/补充后的 units
```

### 核心边界：规则在适配器之外应用

`knowledge::apply()` 是**纯函数**：输入一批 `TextUnit` 与一份规则集，输出过滤/补充后的 `TextUnit`。不碰网络、不碰数据库、不碰适配器内部。

由此得到：
- 19 个格式适配器一行都不用改，全部自动受益
- 可完全独立单元测试
- `--no-knowledge` 立刻回到原行为，出问题能二分定位
- 职责单一：适配器负责「尽可能多提」，知识层负责「减 / 加」

反例（塞进适配器内部）虽省一点开销，但会把学到的规则与硬编码启发式搅在一起，就再也说不清某条文本是被谁排除的。

### 存储

沿用 `profile.rs:596-603` 已确立的约定（`$ATTX_HOME` 优先，否则平台 config 目录）：

```
~/.config/attx/knowledge/
  rmmz.toml       # 已生效规则，按 format_id 分文件
  jsonkv.toml
  proposals.toml  # 待审核提案（含证据）
```

用 TOML 而非 SQLite，是为了让规则可被人读、被 git 管、被手改 —— 这与「默认人工确认」的要求直接契合：审核一个看得懂的 TOML 才现实。

## 规则模型

刻意贫瘠，防止滑向 DSL：

```toml
format = "rmmz"
version = 1

[[rule]]
field = "key"              # 字段名，小写匹配；支持 "*text" 后缀通配
verdict = "skip"           # skip | extract
scope = "nested"           # nested | top | any
confidence = 0.95
reason = "身份句柄，被 gainAchievement() 按原文引用"
evidence = ["お姫さま計画:12次", "别的游戏:5次"]
approved_at = "1753..."
```

匹配只有三个维度：字段名、verdict、scope。**没有**正则、值条件、路径表达式、布尔组合。「除非含汉字」这类条件属于启发式的职责，规则只做确定性分类。

`version` 字段为未来迁移预留，沿用 `store.rs:47` 的容错迁移思路。

### 优先级

workspace 覆盖 > 已批准规则 > 适配器内置启发式。

同层冲突时 **`skip` 胜出**。这个不对称是刻意的：漏译可见且可修，误译身份字段隐形且会坏档。

### 加法的额外闸门

`extract` 规则命中后，仍须通过 `is_machine_literal()` 检查。

理由：`skip` 最坏结果是少提几条，`attx status` 里看得见，人工一眼能发现。`extract` 最坏结果是把开关 ID、文件名、脚本送去翻译 —— 正是今天修复的 bug 形态，且写回后才在游戏里炸。

**规则可以推翻「字段名启发式」，不能推翻「这明显是数字/路径/脚本」。** 学习系统被允许纠正分类判断，不被允许否认事实。

## 证据来源

### 客观信号（零 API 成本）

从 workspace DB 直接统计：

- 提取出来却满足 `is_machine_literal()` → 疑似误提
- 译文与原文完全相同 → 疑似不该翻
- passthrough 聚集在同一字段名 → 模型反复拒绝，疑似非文本
- `quality::check_unit` 拒绝聚集在同一字段名 → 结构性问题

### LLM 复盘（可选）

把候选字段名连同样本值批量交给模型，要一个分类 + 人话理由，把弱统计升级为有解释的提案。

两者都**只产出提案**，批准前不改变任何提取行为。

## CLI

```bash
attx learn scan --workspace <ws>     # 扫描证据，生成提案
attx learn scan --workspace <ws> --llm   # 额外做 LLM 复盘
attx learn review                    # 逐条 y/n
attx learn review --approve 1,3      # 批量批准
attx learn list                      # 当前生效规则
attx learn forget <field>            # 撤销规则
attx extract --no-knowledge          # 逃生舱：回到原行为
```

## 实现范围

新增：
- `src/knowledge.rs` — 规则模型、TOML 读写、纯函数 `apply()`、字段名解析
- `src/learn.rs` — 证据扫描、提案生成、LLM 复盘

改动：
- `src/main.rs` — `attx learn` 子命令组
- `src/pipeline.rs` — `extract()` 中一行调用 + `--no-knowledge` 开关

**19 个格式适配器不改。**

## 测试策略

- `apply()` 是纯函数：规则命中 / 不命中 / 冲突优先级 / 加法闸门，全部可直接单测
- 字段名解析：从 `location` 与 `payload` 两条路径提取字段名，覆盖 rmmz 嵌套路径（`#0/Rewards/0/Name` → `Name`）
- 证据扫描：构造带 passthrough / 同文 / 机器字面量的 fixture，断言提案内容
- 往返：规则写 TOML → 读回 → 语义不变
- 回归：无规则文件时行为与今天完全一致

## 已知风险

**一、行为会随统计而漂移。** 缓解：默认人工确认、规则是人可读 TOML、随时可关、提案强制带证据与样本（而非只给一个置信度数字）。批准一条规则等于替所有未来项目做决定，这一点在 UI 上要说清楚。

**二、加法比减法危险。** 缓解：`is_machine_literal()` 闸门（见上）。
