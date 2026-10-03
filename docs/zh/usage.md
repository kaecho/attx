# 日常用法

先完成[快速开始](quickstart.md)。正常完整任务优先用 `run`，只有需要控制阶段、限额或人工审校时才拆开执行。

## 全量任务

```bash
attx run --input './novel.epub' --src ja --dst zh
```

流水线检测输入、初始化工作区、增量提取、按配置构建术语表、翻译、机械审校和有限修复，再校验源文并写回。最终输出是一份包含各实际执行阶段的 JSON，而不是每条进度行都代表一份结果。

- `--no-translate` 只提取，不调用翻译、不正常写回，也不自动推断 Profile。
- `--no-writeback` 保存译文缓存，但不生成译文文件。
- `--limit N` 限制翻译规模，通常配合 `--no-writeback`；默认完整性检查不会因为做了样本就允许写出未完成文件。
- `--no-glossary` 关闭本轮构建，已有术语仍可注入。`--glossary` 明确开启额外付费构建。
- `--no-infer` 禁止未知格式的模型 Profile 推断，不影响本地 `auto`。
- `--allow-partial` 只用于明确接受部分输出的任务，不应作为绕过失败的默认办法。

## 分阶段运行

```bash
attx detect --input './novel.epub'
attx init --input './novel.epub' --src ja --dst zh
attx extract --workspace './.attx-novel'
attx status --workspace './.attx-novel'
attx translate --workspace './.attx-novel'
attx review --workspace './.attx-novel'
attx writeback --workspace './.attx-novel'
```

`translate --dry-run` 返回计划，不发翻译请求。`writeback --dry-run` 在内存中规范化、校验并生成路径计划，不落盘。两种预览都可选，不是必须取得的额外许可。

只有用户要求样本时使用 `translate --limit 20`。全量任务不需要先试译、等待确认、再重新执行。

## 中断与续译

工作区保存已提交译文。同一输入、引擎和语向再次 `run` 或 `translate` 时，匹配源文的有效缓存无需重复调用模型。网络中断或致命接口错误后先修正原因，再续跑；在途请求可能已发生费用但未完成保存，因此不能承诺中断后完全零额外费用。

```bash
attx status --workspace './.attx-novel'
attx translate --workspace './.attx-novel'
attx repair --workspace './.attx-novel'
```

`pending` 是没有有效缓存的单元，`passthrough` 是模型失败后保留原文的占位。后者不计为成功译文，不应拿 `translated + passthrough` 宣称全译完。`translate --retry-passthrough` 会把占位重新排队；`repair` 也处理占位和其他机械问题。达到修复上限或无进展后，停止重复调用，转人工校对。

输入变化时先重新 `extract`，变化的源文使旧译文失效，消失的单元移除。不能把过期缓存强行写回，也不能编辑数据库的 hash 来规避检查。工作区身份和锁详见[工作区](workspace.md)。

## 人工或离线校对

全量导出可以看见审校报告样例之外的条目：

```bash
attx export-jsonl --workspace './.attx-novel' --output './audit.jsonl' --filter all
# 使用编辑器或审校程序只改 translation / translation_lines
attx import-jsonl --workspace './.attx-novel' --input './audit.jsonl'
attx review --workspace './.attx-novel'
attx writeback --workspace './.attx-novel'
```

JSONL 一行一个对象：

```json
{"id":"chapter.xhtml#p3","text":"こんにちは。","context":"chapter.xhtml","role":"旁白","item_type":"long_text","translation":"你好。","translation_lines":["你好。"]}
```

这是结构示例，不是实际导出结果。保持当前导出的 `id` 和 `text` 不变；导出 ID 是 location，导入也接受对应内部 unit ID。两种别名指向同一单元时仍算重复。

导入先验证整个文件，再通过 SQLite 事务统一保存。未知 ID、源文不符、重复单元、非法结构、行数约束或保护符丢失/重复/错序会使整批失败。空/缺失译文字段可跳过，不计入 `imported`；非空但无效的译文不会被静默忽略。`translation_lines` 存在时优先于 `translation`，不要保留过期数组只改字符串。中文机械残留会规范化；真正日文混入可以导入供修复，但默认写回仍拦截。

过滤器为 `pending`、`all`、`translated`、`passthrough`。`translated` 过滤器按缓存存在性选择，可能包含占位，精确查失败占位用 `passthrough`。

## 外部格式与独立 JSONL

外部提取器应输出至少包含 `id`、`text` 的 JSONL，可加 `context`、`role`、`item_type`。无需工作区：

```bash
attx translate-jsonl --input './source.jsonl' --output './translated.jsonl' --src ja --dst zh
```

独立命令只有内部请求重试，没有工作区自动修复轮和断点续译。输出所有记录并让未完成状态可见，返回 2 不意味着没有生成 JSONL。输出不能覆盖输入；已有输出先备份，再原子替换。外部程序仍负责原格式的安全写回。

需要续译时改用 JSONL 适配器工作区：

```bash
attx run --input './source.jsonl' --src ja --dst zh
# 或目录内提供 source.jsonl，输出 translated.jsonl
attx run --input './text-pack' --engine jsonl --src ja --dst zh
```

## 术语和作品风格

```bash
attx glossary add --workspace './.attx-novel' --src 'アレイ' --dst '艾蕾' --info '女性角色'
attx learn note --workspace './.attx-novel' --name narration --text '旁白使用自然的现代汉语，保留角色称呼差异。'
```

已有译文不会因修改术语或 note 自动重译。术语表负责专名，note 负责明确的翻译要求，preserve 负责不应被翻译的变量与控制串，不要混用。存储结构和作用层见[工作区页](workspace.md)。

## 输出与恢复

文档通常写翻译副本，RMMZ 写 live `data/` 与 `js/plugins.js`，自定义 Profile 取决于 `overwrite`，`auto` 目录输出到 `translated-<dst>/`。以 `paths` 为准，不统一猜文件名。

所有目标先暂存，已有文件先保留首份 `.attxbak`，备份失败不替换。逐文件替换不保证整个目录一起成功。恢复前停止相关操作，按[恢复说明](troubleshooting.md)核对备份和实际路径。

继续：[格式](formats.md) · [质量](quality.md) · [CLI](cli.md)。
