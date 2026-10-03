# 工作区与持久化数据

工作区是某个输入、某个引擎和一个语向的可恢复状态。它不是可随意套到其他作品的译文库。默认目录输入位于 `<content_root>/.attx`，文件输入位于 `<parent>/.attx-<stem>`，可用 `--workspace` 明确指定。

## 文件与身份

| 文件 | 内容 |
|------|------|
| `attx.db` | SQLite meta、units、translations、published，WAL 模式 |
| `workspace.json` | 创建时的可读元数据快照，不是通过编辑它切换身份的入口 |
| `.attx.lock` | 修改命令的操作系统锁载体 |
| `profile.toml` | 自定义/推断规则快照 |
| `glossary.toml` | 本作品术语 |
| `preserve.toml` | 本作品额外保护正则 |
| `experience.toml` | 本作品经验及 prompt note |

元数据包含 engine、game_path、content_root、source_lang、target_lang、created_at，另保存内部源文快照和 Profile 摘要。源和目标语言标签会规范化。已有工作区的内容根、引擎及语向不可改用；init 检测不同输入/语言时要求新目录。Profile 内容改变也要求新工作区；修改工作区规则的 overwrite 标志不能绕过摘要校验。

修改型流水线命令使用 `.attx.lock` 的 OS 锁避免同一工作区并发修改，run 从提取、术语构建到翻译和写回持有同一个工作区锁。进程崩溃后锁自动释放；锁文件存在不等于仍被持有，不需要删除它恢复运行。不要同时给同一个工作区运行两个翻译/写回任务。

## 单元和源文 hash

TextUnit 含 id、engine、domain、location、item_type、role、original_lines、source_line_paths、context、payload。id 由引擎、位置和原文生成 SHA-256 截取标识；独立 source_hash 是原文行的 SHA-256。译文含 unit_id、translation_lines、source_hash、passthrough。

提取事务替换当前单元集合并删除源 hash 不符或已消失的译文，匹配的缓存继续使用。翻译增量保存通过单元，fatal 发生前已提交的有效进度保留。写回重新提取，校验完整源文集合、hash 与锚点，源文新增同样要求重新提取；某些适配器如 auto 另校验源文件 hash。

数据库另有 published 表记录实际发布过的译文行，用于区分缓存修订与已经写到 live 文件的内容。自定义 overwrite Profile 可在 JSONL 修订后安全重复写回，同时保留不相关的 live 键和注释。它不是另一套可手工修改的译文来源，不应清空它来绕过源文校验。

内部 `output_paths` 元数据记录实际生成的文件。自定义目录 Profile 只排除这些已记录副本，不凭语言后缀忽略真正的输入资产。副本目标若与另一个真实源文件冲突，写回会拒绝。旧工作区缺少锚点快照时，执行 `extract --workspace` 刷新；原文 ID/hash 不变的有效缓存仍保留。

pending 是没有有效匹配译文缓存，passthrough 是失败占位，不算成功。review 的残留/控制/姓名问题可能出现在已有译文中，因此 pending=0 不是全量通过。

备份数据库时先停止相关命令，考虑 WAL 状态，不要只在运行中复制一个 attx.db。源文件和规则也应一起保留，不能认为数据库本身包含所有可还原输入。

## 术语表 schema 1

```toml
version = 1
source_lang = "ja"
target_lang = "zh"

[[term]]
src = "アレイ"
dst = "艾蕾"
info = "女性角色"
count = 12
status = "active"
source = "human"
case_sensitive = false
```

term 字段为 src 必填字符串，dst/info/source 默认空，count 默认 0，status 默认 active（另一值 rejected），case_sensitive 默认 false。count 和 min_occurrences 以包含该源词的单元数计，同一单元多行或多次出现只计一次。顶层 version 默认 1，语向默认空；通常由 CLI 填写。用 glossary add/import 修改比手工编辑更容易保持一致。

```bash
attx glossary add --workspace './.attx-book' --src 'アレイ' --dst '艾蕾' --info '女性角色'
attx glossary export --workspace './.attx-book' --file './terms.json'
attx glossary import --workspace './.attx-book' --file './terms.json'
```

JSON 导入支持 `[{"src":"...","dst":"...","info":"..."}]` 或 `{ "原文": "译名" }`。模型构建筛选真实源文子串、出现次数及术语数量；每批只注入相关 active 且有 dst 的术语。rejected 保留以免重复付费询问，list --all 可看。

glossary check 是译名子串审阅辅助，可能误报，不是硬拒绝。构建默认关闭、有费用，最多 40 源批次，不保证全文覆盖；truncated 只报告被 max_terms 截掉的候选，不报告源批次覆盖。min_occurrences=10（实际至少1）、max_terms=200、inject_limit=30 可配置，姓名框候选优先。损坏术语文件会报告 stderr 并按空术语表处理，不应忽略该诊断。

## 保护规则 schema 1

```toml
version = 1

[[rule]]
pattern = '\$[A-Za-z_][A-Za-z0-9_]*'
info = "作品内变量"
```

每条 rule 的 pattern 必填，info 默认空。规则命中在发模型前变为 `[CTRL_n]`，随后还原并验证文字和数量。引擎控制串、markup 与隐式位置 printf 参数必须保持相对顺序；具名或显式编号占位符可随目标语语序调整。重叠命中采用从左到右、最长跨度。非法正则或空匹配拒绝。

内置始终保护 RMMZ 控制串、花括号变量、printf 参数和字面 mask token；Ren'Py 额外保护插值，markup 适配器保护对应内联标签。工作区规则追加而不是替换内置规则。

```bash
attx preserve add --workspace './.attx-book' --pattern '\$[A-Za-z_][A-Za-z0-9_]*' --info '作品变量'
attx preserve list --workspace './.attx-book'
attx preserve remove --workspace './.attx-book' --pattern '\$[A-Za-z_][A-Za-z0-9_]*'
```

保护只适用于真的不应翻译的片段。过宽规则会遮住正文，不能拿它隐藏残留源文。

## 经验 schema 2 与层优先级

```toml
format = "rmmz"
version = 2

[[entry]]
kind = "field"
field = "resourcepath"
verdict = "skip"
scope = "nested"
status = "pending"
domain = "plugins"
confidence = 0.9
reason = "该字段是资源标识而非玩家文本"
evidence = ["作品与命中样本应由真实总结填写"]
source = "learn:auto"
updated_at = ""

[[entry]]
kind = "note"
topic = "prompt"
text = "同一角色的称呼保持一致。"
status = "approved"
source = "learn:agent:voice"
updated_at = ""
```

这是 schema 示例，不是批准建议。优先级从低到高为内置基线、按格式的全局 knowledge、工作区 experience。更高层优先；同层精确字段优于前导 `*` 后缀字段，再冲突时 skip 优于 extract。scope 为 top/nested/any，domain 空表示任何域；field 应为小写。

field entry 的 verdict 为 skip/extract，status 为 pending/approved，其他 confidence/reason/evidence/source/updated_at 提供证据与来源。知识层只过滤适配器已经提取的单元；extract 可以避免其他规则丢弃，却不能凭空创造适配器未产出的文本，也不能复活机器字面值。没有通用表达式语言或任意值谓词。

note entry 的 topic、text、status、source、updated_at 保存明确要求。只有 approved 且 topic=prompt 的非空文本会进入后续翻译系统提示；其他 topic 供人读取。未知 kind 和额外字段保留但不执行。旧 schema 1 的 rule 可以读入，写回使用 schema 2。

全局经验存于 `$ATTX_HOME/knowledge/<format>.toml` 或平台配置目录；不是直接给所有格式共同使用。内置当前有 rmmz 基线，可用 learn defaults 查看。

## 总结、审批与撤回

成功实际写回默认免费总结提取证据，写入该格式的全局 knowledge 文件，可用于未来作品；作品 prompt note 则明确写入 experience.toml。可用 --no-learn 或 auto_summarize=false 关闭本轮总结。llm_review/learn summarize --llm 是额外付费复核，不自动产生文风学习，也不自动批准 skip。

```bash
attx learn summarize --workspace './.attx-book'
attx learn pending
attx learn review --approve 1,3 --reject 2
attx learn list --format rmmz
attx learn list --workspace './.attx-book'
attx learn forget --field resourcepath --format rmmz
```

pending 索引从 1 开始，应先读最新清单和证据再批准。自动 skip 是破坏性的提取移除提案，未经用户批准不生效。代理不能 --approve-all。删除全局字段规则不等于撤掉内置基线；必要时用作品覆盖规则或诊断用 extract --no-knowledge。

作品风格：

```bash
attx learn note --workspace './.attx-book' --name voice --text '旁白不用网络流行语。'
attx learn forget --workspace './.attx-book' --name voice
```

同名 note 更新，名字保存在 source 标记里。全局 note 必须明确选择 --format，因为它影响未来作品。新增 note/术语不自动重译已完成单元。

## 写回与恢复边界

规范化/固定槽位重排在内存完成，成功写文件后才保存变更译文缓存和发布记录。blocked 和 dry-run 不改缓存或输出。所有产物先暂存，首份现有目标追加 `.attxbak`，备份失败停止；之后原子替换每个文件，而非目录事务。输出继承适用的源文件权限，私密暂存/备份在 Unix 使用0600；目录句柄约束文件操作，避免提交阶段重新解析可被替换的目录路径。

恢复前核对 paths、备份来源和当前文件，不直接改 attx.db 或 workspace.json。详细步骤见[故障排查](troubleshooting.md)。继续：[质量](quality.md) · [架构](architecture.md)。
