# 可扩展经验层与术语表

日期：2026-08-02
状态：已批准，待实现
目标版本：v0.6.0

## 背景

v0.5.0 落地了提取知识层（`knowledge.rs` + `learn.rs`）：从工作区证据挖提案，人工 `attx learn review --approve` 后成为规则，在适配器之外纯函数过滤 units。

它有三个够不到的地方：

1. **证据挖掘是手动的**。`attx learn scan` 得有人记得跑。翻译完就走人的用户，经验永远沉淀不下来。
2. **规则模型只能表达一件事**：某字段该不该提取。「这个格式的控制码容易被吞」「这类作品的敬称要保留」这类经验无处安放。
3. **默认提取判断仍写死在适配器里**（`rmmz_plugins.rs` 的 `visible_key()` 白名单 / `skip_key()` 黑名单）。agent 看不见，也就改不了。

同时缺一个跨批次的术语一致性机制——README roadmap 里的「跨批次术语表 / 译名固定」。没有它，长篇作品里同一角色名会在不同批次被译成不同名字。

## 目标

- 写回后**自动**沉淀经验，下次同格式提取前自动读取
- 经验文件格式可扩展：attx 认识的 kind 机械执行，不认识的原样保留并暴露给 agent
- 默认提取规则下沉为随发行的数据文件，agent 可读可改
- 术语表：统计剪枝 + LLM 命名 + 按批注入 + 译后回检，默认关闭

非目标：

- 不构建规则 DSL（正则、值条件、布尔组合一律不做，沿用 v0.5.0 决定）
- 不改 19 个格式适配器的提取逻辑
- 术语表不做跨项目共享（理由见下）

---

## 一、经验层

### 1.1 文件格式

单一 `[[entry]]` 数组 + `kind` 判别字段，**不用多张具名表**。

```toml
# ~/.config/attx/knowledge/rmmz.toml   （沿用现有路径，不破坏已有安装）
format = "rmmz"
version = 2

# ── kind="field"：字段名级提取判断，attx 机械执行 ──
[[entry]]
kind       = "field"
field      = "key"          # 小写匹配；前导 * 为后缀通配（*text）
verdict    = "skip"         # skip | extract
scope      = "nested"       # nested | top | any
status     = "pending"      # approved | pending
confidence = 0.95
reason     = "身份句柄，被 gainAchievement() 按原文引用"
evidence   = ["お姫さま計画: identical=6/6"]
source     = "learn:auto"   # builtin | learn:auto | human | agent
updated_at = "1754..."

# ── kind="note"：自由经验，自动生效 ──
[[entry]]
kind   = "note"
topic  = "prompt"           # prompt | extraction | writeback | format | <agent 自创>
text   = "本格式 \\C[n] 颜色码常在句首，模型易吞；已由质量层拦截"
status = "approved"
source = "learn:auto"

# ── attx 不认识的 kind：原样保留，暴露给 agent ──
[[entry]]
kind      = "voice-hint"
character = "アレイ"
note      = "关西腔，译文轻微口语化"
```

**为什么是单数组 + 判别字段。** 「agent 能自己加条目」这件事，如果用多张具名表实现，attx 必须提前知道表名才能在读写往返中保住它；新增一类经验就得改 schema。单数组下，新增 kind 是零 schema 变更操作。

**往返保留是硬要求。** 反序列化进刚性 struct 会让 attx 在下次写回时静默丢掉不认识的字段，agent 写的东西就蒸发了——「可扩展」会变成名义上的。实现约束：

- 已知 kind 的 entry 用 `#[serde(flatten)] extra: BTreeMap<String, toml::Value>` 接住未知字段
- 未知 kind 的整条 entry 保留为原始 `toml::Value`，不解析
- 必须有单元测试断言：读入含未知 kind 的文件 → 写回 → 语义不变

### 1.2 生效链

```
适配器内置启发式（代码，兜底）
  ← 内嵌默认经验（include_str!，source="builtin"）
  ← ~/.config/attx/knowledge/<format>.toml     学到的 + 人工
  ← <workspace>/experience.toml                本项目覆盖
```

后者覆盖前者。同层冲突 **`skip` 胜**——沿用 v0.5.0 的不对称：漏译在 `attx status` 里看得见且可修，误译身份字段坏档且无声。

`extract` 规则命中后仍须过 `knowledge::is_machine_literal()` 闸门。**允许纠正字段名判断，不允许否认值本身是数字 / 路径 / 脚本。**

**默认经验用 `include_str!` 内嵌进二进制，不作为松散文件分发。** 它由适配器现有的字段名白/黑名单导出，与适配器代码是同一份事实；作为可丢失的外部文件分发只会带来「文件没打进包」和「与代码不同步」两类故障，还得为 `$ATTX_HOME` / 可执行文件同级 / 仓库目录写一套查找顺序。

内嵌不妨碍 agent 修改——它改的本来就该是覆盖层。为此提供：

```bash
attx learn defaults --format rmmz    # 把内嵌默认经验打到 stdout，供 agent 阅读或另存为覆盖层起点
```

适配器代码不动：默认条目经同一条合并链参与 `knowledge::apply()`，19 个格式零回归风险，agent 也第一次能看见并覆盖内置基线。

### 1.3 生效门槛

| kind / verdict | 写入时 status | 理由 |
|---|---|---|
| `note` | `approved` | 自动生效。最坏结果是提示词多几句话 |
| `field` + `extract` | `approved` | 加法，不丢数据，且过机器字面量闸门 |
| `field` + `skip` | **`pending`** | 会真的丢文本，需人工 `attx learn review --approve` |

这是「自动改进」与「无声漏译」之间的分界。加法自动，减法要批准。

### 1.4 自动总结

`pipeline::writeback()` 成功后调用 `learn::summarize(workspace)`，复用现有 `collect()` 的客观信号（machine / identical / passthrough 按字段名聚集），**零 API 成本**：

- 产出 `kind="field"` 条目：`skip` 落 `pending`，`extract` 落 `approved`
- 产出 `kind="note"` 条目：passthrough 率、最常见质量拒绝原因、控制码丢失计数
- `[learn] llm_review = true` 时才额外调模型深化（默认 false）

`attx learn summarize --workspace <ws>` 手动触发同一函数。

逃生舱：`attx writeback --no-learn`、`attx extract --no-knowledge`。

### 1.5 note 的消费方

- **attx**：`topic = "prompt"` 的 note 追加进翻译系统提示词
- **agent**：`attx learn list` 输出全部 entry（含未知 kind），供 agent 在提取前决策

---

## 二、术语表

### 2.1 存储与条目

`<workspace>/glossary.toml` —— **按项目存，不进全局知识库**。角色名是作品专有的：游戏 A 的 `アレイ` 与游戏 B 的 `アレイ` 无关，混进跨项目经验文件会污染所有后续项目。

```toml
version = 1
source_lang = "ja"
target_lang = "zh"

[[term]]
src    = "アレイ"
dst    = "艾蕾"
info   = "女性名字"
count  = 47
status = "active"        # active | rejected
source = "auto"          # auto | human | import
```

`info`（消歧描述）是质量关键，不是可选装饰。没有它，模型不知道 `アレイ` 是女性名，`アレイさん` 就可能译成「埃雷先生」——同一角色在故事里一会儿男一会儿女。

### 2.2 四阶段闭环

```
extract  →  glossary build  →  translate  →  writeback  →  glossary check
             ①挖候选 ②卡阈值            ④按批注入                ⑤回检生效
             ③LLM 命名
```

**① 挖候选（零成本正则）**

| 源语言 | 模式 |
|---|---|
| ja | 片假名串 `[\u{30A0}-\u{30FF}]{2,}`、汉字串 `[\u{4E00}-\u{9FFF}]{2,6}` |
| en | 连续大写词 `\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*\b` |

预过滤：长度 1、纯平假名、`knowledge::is_machine_literal()` 命中者直接丢。

**② 卡阈值**：`count >= min_occurrences`（默认 10），再按频次降序截断到 `max_terms`（默认 200）。

被截断的条数**必须打印**。静默截断会被读成「已全覆盖」。

**③ LLM 命名**：幸存者分批（40 条/请求）送模型，要 `[{src, dst, info, keep}]`。

`keep: false` 是这一步的核心。统计一定会捞出「自分」「今日」这类高频非专有名词——LinguaGacha 的术语表 wiki 把它列为头号反面案例（「春」太常见了，不是专有名词）。模型有权否决候选，落 `status="rejected"` 而非删除，避免下次 build 反复重提。

成本量级：200 候选 ≈ 8k 字符 ≈ 5 次请求。

**④ 按批注入**：`Translator::translate_batch()` 扫描本批原文，只注入**真实出现**的 active 术语（上限 `inject_limit`，按频次排序），插在 `# 正文` 之前：

```
# 术语表
アレイ → 艾蕾（女性名字）
エルギア国 → 埃尔迦国（地点）

# 正文
```

放 user body 而非 system prompt：`Translator::new()` 只构建一次 system，术语表逐批不同。

**⑤ 回检**：`glossary check` 对每个 active 术语找出原文含 `src` 的单元，检查译文是否含 `dst`，报告未生效条目与比例。子串检查对屈折语不完美，但与 LinguaGacha 同级，够用。

### 2.3 配置

```toml
[glossary]
enabled         = false   # 默认关闭：构建术语表会产生额外 LLM 费用
min_occurrences = 10
max_terms       = 200
inject_limit    = 30

[learn]
auto_summarize  = true    # writeback 后自动总结（零 API 成本）
llm_review      = false   # 额外让模型复核提案（有费用）
```

`enabled` 只管**自动构建**（`attx run` 是否插入 build 阶段）。若 `glossary.toml` 已存在（手动 build 过或 import 了现成表），`translate` 一律注入——注入几乎不花钱，不注入才是意外。显式 `attx glossary build` 不受该开关限制：显式调用即同意。

### 2.4 CLI

```bash
attx glossary build  --workspace <ws> [--min-occurrences N] [--dry-run]
attx glossary list   --workspace <ws>
attx glossary add    --workspace <ws> --src X --dst Y [--info Z]
attx glossary remove --workspace <ws> --src X
attx glossary import --workspace <ws> --file g.json
attx glossary export --workspace <ws> --file g.json
attx glossary check  --workspace <ws>
attx learn  summarize --workspace <ws>
attx learn  defaults  --format <id>
attx writeback --no-learn
```

`import` 兼容两种 JSON：`[{"src","dst","info"}]` 与 `{"src": "dst"}`。

### 2.5 配置向导新增项

SKILL.md 阶段 -1 增加第 6 项：

> **术语表**（默认关闭）：是否开启自动术语表？开启后 attx 会在翻译前额外调用模型，为高频专有名词（人名/地名/组织名）统一译名，显著提升长篇作品的一致性，但会增加约 5–10 次额外请求的费用。若开启，最低出现次数默认 10——调低能收录更多术语，费用与噪音同时上升。

---

## 实现范围

新增：

- `src/glossary.rs` — 术语模型、TOML 存储、候选挖掘、LLM 命名、注入选择、回检
- `src/defaults/<format>.toml` — 由适配器字段名表导出的默认经验条目，`include_str!` 内嵌

改动：

- `src/knowledge.rs` — entry 模型（含未知 kind 保留）、v1→v2 兼容读取、分层合并、内嵌默认层
- `src/learn.rs` — `summarize()`；提案改产 entry；skip 落 pending
- `src/config.rs` — `[glossary]`、`[learn]` 两节 + `example_toml()`
- `src/llm.rs` — 按批注入术语表；`topic="prompt"` 的 note 进系统提示词
- `src/pipeline.rs` — writeback 后自动 summarize；`run` 在 extract 与 translate 间插入 build（enabled 时）
- `src/main.rs` — `glossary` 子命令组、`learn summarize`、`learn defaults`、`--no-learn`
- `skills/attx/SKILL.md` + `references/cli-command-contract.md` — 向导第 6 项、新命令契约
- `README.md` / `README.zh-CN.md` — 术语表与经验层章节；roadmap 划掉「跨批次术语表」

**19 个格式适配器不改。** 发行流程不改（默认经验内嵌，无新增打包产物）。

实现顺序：经验层（第一部分）与术语表（第二部分）互不依赖，可分两批落地并各自独立验证；建议先经验层——它改动 `knowledge.rs` 的核心数据模型，先稳住再叠术语表。

## 测试策略

经验层：

- 未知 kind 往返保留（读→写→语义不变）——最容易做成假的一条
- 分层合并优先级：builtin < 全局 < workspace；同层 skip 胜
- `status="pending"` 的 skip 不生效；`note` 与 `extract` 生效
- v1（`[[rule]]`）文件能被 v2 读取并按 approved 处理
- 每份内嵌 `defaults/*.toml` 都能解析（防止手写默认表打错字到运行时才炸）
- 无经验文件时行为与今天完全一致（回归）

术语表：

- 候选挖掘 ja / en fixture；阈值过滤；截断计数被报告
- LLM `keep=false` 落 rejected 且下次 build 不重提
- 注入只选本批真实出现的术语，且受 `inject_limit` 限制
- 回检能发现未生效术语
- JSON 两种输入格式导入 + 导出往返
- `glossary.enabled=false` 且无 `glossary.toml` 时 translate 路径不变（回归）

## 已知风险

**一、自动生效的 note 会漂移提示词。** 缓解：note 是人可读 TOML、`attx learn list` 可见、可删；只有 `topic="prompt"` 进提示词，其余仅供 agent 阅读。

**二、术语表错译会被系统性放大。** 一条错的 `dst` 会在全作品所有批次生效，比零散错译更难发现。缓解：`glossary check` 回检、`attx glossary list` 可审、`add`/`remove` 可改、`info` 让模型有机会自我纠偏。

**三、统计挖掘对日文没有词边界。** 片假名串与汉字串会切出非词的片段。缓解：`min_occurrences` 阈值先剪，LLM `keep` 再否决；两道闸门都不通过的候选不会进表。
