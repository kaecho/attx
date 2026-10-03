# 架构

attx 是一个 Rust 2024、MSRV 1.89 的单二进制程序，不提供异步运行时或独立 Python 服务。CLI、代理和手动用户共用同一条流水线，格式处理不发网络请求。

## 数据流

```text
main.rs：解析命令与结果码
  → config.rs：模型与流水线配置
  → pipeline.rs：检测、工作区、提取、翻译、审校、写回
      → adapter/* 或 profile.rs：格式到 TextUnit，译文到 OutputFile
      → store.rs：SQLite 缓存与源文身份
      → llm.rs：批次、上下文、保护、同步 HTTP、有限重试
      → quality.rs / review.rs：规范化、结构检查、报告与修复候选
      → fileio.rs：锁、暂存、备份和逐文件替换
```

一个适配器只负责检测、提取和内存渲染。`writeback` 返回 OutputFile（路径、字节及可选 permissions），真正的备份/提交由流水线负责，避免每个格式各写一套安全流程。fileio 使用 cap-std 目录句柄约束暂存、备份和 rename，避免可变父路径在提交时被重新解析。

## 模块职责

| 模块 | 维护重点 |
|------|----------|
| [`main.rs`](https://github.com/kaecho/attx/blob/main/src/main.rs) | clap 命令树、参数别名、stdout JSON、exit 0/1/2，run 编排 |
| [`config.rs`](https://github.com/kaecho/attx/blob/main/src/config.rs) | serde TOML schema、默认值、文件搜索、客户端选择 |
| [`model.rs`](https://github.com/kaecho/attx/blob/main/src/model.rs) | TextUnit/Translation/WorkspaceMeta/JsonlRecord、hash、源语言启发式 |
| [`pipeline.rs`](https://github.com/kaecho/attx/blob/main/src/pipeline.rs) | 工作区身份、提取事务、有限修复、源文校验、writeback 报告、Profile 推断 |
| [`store.rs`](https://github.com/kaecho/attx/blob/main/src/store.rs) | SQLite meta/units/translations、WAL、批次提交、源 hash 失效处理 |
| [`llm.rs`](https://github.com/kaecho/attx/blob/main/src/llm.rs) | 同步 Chat Completions、线程共享限速、上下文预算、请求重试及致命停止 |
| [`quality.rs`](https://github.com/kaecho/attx/blob/main/src/quality.rs) | 结构校验、中文机械尾屑清理、保护片段和语言脚本信号 |
| [`review.rs`](https://github.com/kaecho/attx/blob/main/src/review.rs) | 全量机械检查、最多40条样例桶、唯一修复候选 |
| [`preserve.rs`](https://github.com/kaecho/attx/blob/main/src/preserve.rs) | 内置/工作区正则、遮罩还原、数量及相对顺序检查 |
| [`glossary.rs`](https://github.com/kaecho/attx/blob/main/src/glossary.rs) | 术语存储、可选模型提取、真实子串/次数门槛、相关术语注入、建议检查 |
| [`knowledge.rs`](https://github.com/kaecho/attx/blob/main/src/knowledge.rs) | 内置/全局/作品经验层、field/note schema、纯过滤和机器字面值保护 |
| [`learn.rs`](https://github.com/kaecho/attx/blob/main/src/learn.rs) | 证据总结、待批准 skip、note 与审批 |
| [`profile.rs`](https://github.com/kaecho/attx/blob/main/src/profile.rs) | 声明式规则编译、动态适配器、用户规则目录与工作区快照 |
| [`fileio.rs`](https://github.com/kaecho/attx/blob/main/src/fileio.rs) | OS workspace lock、同目录暂存、首份备份、原子单文件替换 |
| [`textio.rs`](https://github.com/kaecho/attx/blob/main/src/textio.rs) | 通用文本解码，auto 单独保证原编码可表示性 |

适配器登记在 [`adapter/mod.rs`](https://github.com/kaecho/attx/blob/main/src/adapter/mod.rs)，XML 小工具在 xmllite，RMMZ 插件参数逻辑在 rmmz_plugins，未知文本字节跨度逻辑在 auto。

## 数据模型

TextUnit 不是文件的一行。它可能是多行对白、段落、单元格或字符串数组。item_type 有 long_text、array、short_text；array 必须保持数组行数，short_text 只有一项，long_text 可以按语言调整断句，但写回适配器可能有额外槽位约束。

location 是可审阅的位置，source_line_paths 是行锚点，context 用于场景分组，payload 保存适配器自己的结构信息。引擎、位置和原文决定 id，原文另有 source_hash。译文不能只凭 location 套到变化后的源文。

JSONL 导出以 location 作为 id，内部数据库用 unit id。导入先解析全文件、验证 alias 对应唯一单元及当前 text，再一个事务提交；模型、手工导入和旧缓存共用保护与中文规范化路径。

SQLite 的 published 表记录已经写到输出的译文行，使 overwrite Profile 能校验已发布状态后应用 JSONL 修订，并保留不相关的 live 键/注释。该记录与待修订 translations 不同，不能把尚未写出的缓存误当成源文件现状。

units 表保存提取序号 ordinal，让不透明 JSONL ID 也保持原始顺序；数字定位符在场景内自然排序。已接受的同批条目在失败条目完成重试之前就增量入库。API Key 不作为翻译内容发送；响应内容若含配置密钥则拒绝，HTTP 错误中的匹配凭据会脱敏。

## 线程与失败边界

HTTP 使用 reqwest blocking，批次由标准库 scoped threads 执行。worker_count 限并发，rpm 是共享请求开始时钟，retry_count 限每个单元每个 pass 的请求，repair_rounds 限工作区后处理轮数。不是 key rotation、无限队列重试或供应商自动切换。

格式失败缩小到失败单元，已通过 ID 保存一次。408/429/5xx 和网络问题有限重试；永久 4xx 触发致命停止并抑制排队请求，但不能取消所有已在途请求。先前已保存批次保留。

review 报告的样例上限仅影响展示，修复候选使用完整集合。术语建议不应变成无限重译门禁。真实日文必须作为未解决文本报告，而不是被清理函数删除。

## 安全写回不是目录事务

工作区绑定身份，run 持有整条修改流水线的锁。提取保存源文快照，写回校验完整集合、内容 hash 和锚点，新增源文也须重新提取。自定义快照摘要防止改规则或 overwrite 后绕过原选择。

写回先在内存中规范化和校验，默认不允许 pending/无效单元，然后所有产物先暂存、所有必要备份完成，再逐文件替换。任一备份失败不开始替换；后续 commit 失败时可能有先前文件已更新，因此恢复需要 `.attxbak`，不能承诺整个目录回滚。

成功实际写回之后才保存规范化/重排译文。dry-run、blocked 不改缓存和输出。部分写回仅写有效子集，未解决位置使用原始源文，即使 live 游戏此前已有旧译文也不能留成误导的新结果。

## 设计来源与非目标

失败单元重试、编号遗漏检查、严格结构验证、有限邻文和共享代理批次流程参考了 LinguaGacha v0.125.0 的设计思路。attx 独立用 Rust 实现，没有直接复制其源代码，也不据此推定许可兼容。`auto` 和声明式 Profile 推断扩展的是 attx 自身的格式机制，不应说成上游已有任意格式推断。

当前没有 Electron UI、Pi 运行时、PDF 处理、OCR、供应商 key rotation 或上游动态并发算法。保持同步实现与清晰边界比增加第二条代理特供流水线更重要。

继续：[二次开发](development.md) · [落盘 schema](workspace.md)。
