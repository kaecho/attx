# 质量与审校

attx 的机械检查能发现结构问题和部分明显漏译，不能评价整部作品的准确性、文风或剧情理解。默认流程做有限修复后报告剩余项，不承诺一次完美。

## 翻译结果如何验证

模型返回的批次按请求编号对应。未知编号丢弃，重复编号不能提高成功数，缺失或不合格单元有限重试。输出截断、行数不符、空内容、控制标记丢失/重复/相对顺序改变等不能靠填回原文伪装成功。失败耗尽可保留原文作 passthrough，占位不计成功。

所有保护片段都校验精确数量。引擎控制串、markup 和隐式位置 printf 参数还必须保持相对顺序；具名或显式编号的占位符可随目标语语法重新排序。独立姓名标签及其正文不能在模型输出中遗漏。

上下文受字符预算、文件/场景边界和自然位置顺序限制。系统提示要求准确目标语言、保留结构及变量、使用相关术语和已有风格 note，但提示词不保证模型遵守。

## 中文机械尾屑清理

模型、JSONL 导入及已有中文缓存共用规范化规则。只有足够明确的中文上下文才清理机械假名残留：孤立 `っ`/`ッ` 去除，`っすよ`/`っす` 可转为 `哦`，机械长音 `ー` 转为 `～`。

以下是源码测试用例规定的输入与结果，不是实测模型报告：

| 输入 | 规范化结果 |
|------|------------|
| `唔，不要，好、好羞耻……っ」` | `唔，不要，好、好羞耻……」` |
| `啊呜……ッ` | `啊呜……` |
| `各位请务必小心っす。` | `各位请务必小心哦。` |
| `没问题っすよ` | `没问题哦` |
| `哟，奥伦在吗ー？` | `哟，奥伦在吗～？` |
| `啊ー太舒服了` | `啊～太舒服了` |
| `咯咯ーー` | `咯咯～～` |

分隔符 `・` 保留，例如 `・禁止挑食`、`乔・约翰尼`。保护片段内部不改，如 `\SE[カナ]`。完整日文 `今日はいい天気っ` 和姓名 `アーレン` 不删；`反弓成「く」字形`、引用中描述 `っ` 的字形含义也不机械删除。`啊にっ` 仍含实质日文，不能假装清理完成。

规则支持规范化后的中文语言标签，包括常见简繁中文别名。它不是任意假名删除器，不负责语义翻译，也不能用正则把所有假名去掉来减少报告数量。非中文目标不套用这些中文转换。

日中共用的纯汉字标签，例如 `魔法`、`通信`，不会只因译文与原文相同就被强制判为漏译。否则正常 UI 标签也会耗尽重试并阻止写回；共用文字的语义是否正确仍需要审阅。

## 审校报告

```bash
attx review --workspace './.attx-novel'
```

不调用模型。每个 bucket 是 `{count, sample:[{location,unit_id,detail}]}`，sample 最多 40 条。count 是完整计数，不受样例上限截断。示意结构如下，其中 detail 文本按程序现有英文字串表示，路径及 ID 仅用于说明：

```json
{
  "total": 3,
  "translated": 2,
  "pending": 0,
  "passthrough": 1,
  "glossary": {"active_terms": 0, "terms_seen": 0, "terms_fully_applied": 0, "violations": []},
  "residual_source": {"count": 2, "sample": [
    {"location": "scene#2", "unit_id": "example-mixed", "detail": "translation mixes target text with Japanese kana"},
    {"location": "scene#3", "unit_id": "example-original", "detail": "Japanese text remains untranslated"}
  ]},
  "kana_edge": {"count": 0, "sample": []},
  "kana_mixed": {"count": 1, "sample": [
    {"location": "scene#2", "unit_id": "example-mixed", "detail": "translation mixes target text with Japanese kana"}
  ]},
  "kana_untranslated": {"count": 1, "sample": [
    {"location": "scene#3", "unit_id": "example-original", "detail": "Japanese text remains untranslated"}
  ]},
  "identical": {"count": 1, "sample": [
    {"location": "scene#3", "unit_id": "example-original", "detail": "translation identical to source"}
  ]},
  "control_loss": {"count": 0, "sample": []},
  "namebox_mismatch": {"count": 0, "sample": []}
}
```

| bucket | 应如何处理 |
|--------|------------|
| `residual_source` | 聚合残留源语言信号，包括需要翻译却相同的原文和 passthrough |
| `kana_edge` | 中文边缘机械残留；先看规范化结果，不能扩大成删句 |
| `kana_mixed` | 中日混杂或语义字形引用等；人工判定，不自动删假名 |
| `kana_untranslated` | 仍是日文片段或日文原文占位；需翻译/修复 |
| `identical` | 需要翻译的原文原样复制；不算完成 |
| `control_loss` | 保护符/变量丢失，应修复，不能关闭保护蒙混通过 |
| `namebox_mismatch` | 同场景姓名框与正文姓名用法不一致；检查称呼和上下文 |

三个 kana 分类对同一单元互斥，但会和聚合残留及 identical 等重叠。不能把所有 count 相加当作唯一问题单元数。源语言检测包含常见 Latin、CJK、Hangul、Cyrillic、Arabic、Devanagari、Thai 的启发式，不是真正的语言识别。引用、专名、中文与日文共有汉字可能误报或漏报。

`glossary.violations` 按译名子串检测，屈折语言、别名和上下文变化可造成误报，因此仅作建议，不是无限重译的硬门禁。给出 `src`、`dst`、`occurrences` 和 `applied` 供人判断。

## 有限修复

```bash
attx repair --workspace './.attx-novel' --dry-run
attx repair --workspace './.attx-novel'
```

修复候选包括 pending、passthrough、源文残留、控制符和姓名框问题，候选集合不受 report 40 条样例上限影响。`translate`/`run` 最多附加配置的修复轮数；显式 repair 至少一轮。HTTP 重试和修复轮不是同一个设置，准确预算见[配置](configuration.md)。

达上限或无进展后，报告 `unresolved`，再用全量 JSONL 人工审校。不要不断重新运行 repair 把有限预算变成无限调用。默认 writeback 拦截待译和无效单元；明确使用 `--allow-partial` 才写有效子集，未解决位置保留原文，返回 2。

## 写回规范化与布局

writeback 在内存中规范化中文缓存并重排 RMMZ 固定槽位，然后 review、验证源文、暂存和替换。实际成功写回后才把变更译文保存回缓存。dry-run 和 blocked 不改缓存或输出。

`normalized_lines` 是规范化行数信号，`reflowed_units` 是 RMMZ 重排单元数，`overflow_lines` 是超过 44 半角显示单元的槽位信号。姓名首槽保留，正文长到剩余槽位无法容纳时不截断，报告最后槽溢出。溢出本身不证明运行时一定裁字，但必须试玩，不能静默忽略。

## 完成摘要应包含什么

报告真实输出路径、是否实际写回、已译、pending、passthrough、unresolved、覆盖遗漏和布局溢出。主动样本限制、dry-run、no-writeback 应明确说明。机器 status 为 ok 仅代表本次规则下完成，不能替代人工语义审校或游戏试玩。

继续：[CLI 报告字段](cli.md) · [RMMZ 布局](rmmz.md) · [人工校对](usage.md)。
