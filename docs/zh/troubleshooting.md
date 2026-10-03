# 故障排查

先保留工作区与原始输入，不用删库、跳过校验或直接编辑输出解决一般失败。最终 stdout JSON、退出码和 stderr 错误分别保存；对外报告前去掉密钥、带凭据 URL、私密原文和完整配置。

## 配置和 HTTP

| 现象 | 处理 |
|------|------|
| 找不到 client | 检查显式 --config 路径、当前目录、default_client 与 name，不能假定二进制旁或平台配置目录会自动加载 |
| doctor 顶层 ok 但没有连接 | 查看 llm.configured 与 ping；连接失败也可能返回 0 |
| 401/403 | 修正本机 key、权限或端点后再运行，不能反复重试、轮换密钥或继续 glossary |
| 400/错误模型/不支持参数 | 核对 model 与 base_url，删去服务不支持的 temperature/reasoning_effort/max_tokens/extra 字段；provider_type 不改变协议 |
| 404 | base_url 不要重复包含 /chat/completions，确认接口路径而非网页地址 |
| 429 | 降低 worker_count/rpm，按服务额度执行；工具仅有限重试，不能无穷循环 |
| 408、5xx、网络中断 | 等候/修正网络后用同工作区续译；早先已提交译文保留 |
| 超时、输出截断 | 控制 batch_chars/max_context_items、核对输出 token 限额和 timeout；截断不能当成功导入 |
| SSE 无内容 | 确认服务按 delta.content 提供流，必要时使用 stream=false |

永久 4xx 会停止排队请求，在途 HTTP 仍可能结束，不保证没有任何后续费用。错误本身不是重复 ping 的理由。

## 检测或提取

```bash
attx formats
attx detect --input './input'
attx analyze --input './input' --src ja
```

检测错误可以通过 run/init 的 --engine 或 --profile 明确选择；detect 不支持 --engine。run 零单元会停止，先核对源语言、规则范围与是否主要为机器数据。

未知文本依次考虑 auto、保存 Profile、保守声明式推断。需要禁止模型推断费用时加 --no-infer。二进制、加密或复杂脚本转义需要可靠外部提取器和 JSONL，不运行模型生成的代码。

auto 目录不会再次分派专用 EPUB/字幕/脚本适配器，unsupported/copy 数量不是翻译数。最多50个 unsupported_paths 只是样例。Profile 能提取不意味着任意目标文字能安全嵌回，roundtrip 应检查具体字段。

## 编码错误

auto 拒绝有损解码，保留原编码，目标字符不可表示时会失败。使用外部编辑器制作 UTF-8 源副本，明确新输入及新工作区再运行，保留原始文件。不能靠替换成问号、删中文字或更改 source_hash 蒙混通过。

Profile 和多数专用文本适配器输出 UTF-8，不要拿 auto 的编码保证套到所有格式。

## 工作区 busy 或身份不符

busy 表示同工作区有修改命令持锁。等待或正常终止你自己启动的相关进程。OS 在退出/崩溃后释放锁；`.attx.lock` 文件残留无需删除。run 持有从提取到写回的整个修改锁。

身份不符表示输入内容根、引擎、语言或 Profile 快照不同，使用新的 --workspace 目录。不要修改 workspace.json、数据库 meta 或 profile 摘要去骗过验证，也不借其他作品缓存。

## source changed

写回校验提取快照、原文 hash 与锚点，新增源文同样要求重新提取。发生变化时：

```bash
attx extract --workspace './.attx-book'
attx status --workspace './.attx-book'
attx translate --workspace './.attx-book'
attx writeback --workspace './.attx-book'
```

匹配的译文保留，变化项失效。游戏版本大幅变化时使用新工作区更容易保持边界。RMMZ 提取可用 data_origin/ 或 .attxbak 作为源，检查这些源快照是否确属当前版本，不把已翻译 live data 误作源文。

## needs_attention、blocked 或 exit 2

```bash
attx status --workspace './.attx-book'
attx review --workspace './.attx-book'
attx repair --workspace './.attx-book' --dry-run
attx repair --workspace './.attx-book'
```

pending、passthrough、真实残留、控制符或姓名框问题可使默认写回 blocked。有限修复用完或无进展时用 export-jsonl --filter all 人工校对；不能无穷重跑来隐藏问题。术语子串建议可能误报，不能因此机械重译全项目。

主动试译 --limit N 时退出2可能只是剩余未处理条目，不代表缓存丢失。继续全量或明确保留样本，不能宣称已经完成。dry-run 返回0也可能携带 blocked 计划，未写文件。

只有用户明确接受部分结果时才用 --allow-partial：写有效子集，未解决位置恢复/保留原始源文，即使先前 live 文件有旧译文也不能沿用它冒充本轮成功。状态仍 needs_attention、退出2。

## JSONL 导入失败

| 现象 | 处理 |
|------|------|
| unknown id 或 source text mismatch | 重新从当前工作区导出，保留当前 id/text，仅改译文字段 |
| duplicate unit | 删除重复记录；location 和内部 hash ID 指同一单元也重复 |
| 格式/行数错误 | array 保持行数，short_text 一项，检查 JSON 数组类型和空内容 |
| preserve 丢失/错序 | 恢复控制符/变量的内容、数量和相对顺序，不删掉保护规则 |
| 只改 translation 没生效 | translation_lines 存在时优先，确保两者一致或只保留有效数组 |
| imported 比记录数少 | 空/缺失译文跳过；非空无效内容则整批拒绝，不静默丢弃 |

整文件先验证再一个事务保存，失败不留半导入状态。中文尾屑可规范化，但真正日文残留保留并进入后续修复，默认写回仍检查。

## 中文或姓名框仍有问题

不能把所有假名删掉。完整日文、姓名、语义字形引用和保护串不机械清理，详情见[质量页](quality.md)。过宽 preserve 会隐藏漏翻，应只保护机器片段。

姓名框与正文不同译名时先维护 glossary，再定向修复或人工 JSONL 校对。修改术语/note 不会自动重译已完成条目。RMMZ 首槽说话人不能被正文挤占，固定槽位不增加事件指令；最后槽的 overflow_lines 需要试玩或人工缩短译文，不裁掉句尾。

## 备份和替换失败

先确认输出目录可写、磁盘空间、文件未被其他程序占用，以及备份是普通文件。不要删除备份或关闭校验作为第一反应。备份失败时不会开始本批目标替换。

输出全部暂存后逐文件提交，commit 中途失败可能有先前文件已改。检查错误所示阶段和 paths，核对已有 .attxbak 恢复必要文件；这不是整个目录事务，也没有自动目录回滚承诺。

### 恢复一个明确文件

停止 attx 和游戏，保存需要保留的当前译文，核对备份确是该目标的原始文件后，手动恢复一个确切路径：

Linux/macOS：

```bash
cp './game/data/Map003.json.attxbak' './game/data/Map003.json'
```

PowerShell：

```powershell
Copy-Item '.\game\data\Map003.json.attxbak' '.\game\data\Map003.json' -Force
```

备份名称是完整文件名追加 .attxbak，不是把 .json 改成 .attxbak。首份备份不会被后续译文覆盖。不要递归批量恢复未经核对的其他作品文件。恢复 live data 不会删缓存，下次 writeback 仍可能重新应用译文；需要检查恢复目的和后续命令。

## 文档或开发失败

源码构建需要 Rust 1.89+。文档用 requirements-docs.txt 和 mkdocs build --strict，不编辑生成 site/。命令、配置与翻译流程扩展见[二次开发](development.md)。提交问题时附版本、无敏感值的命令、引擎/格式、退出码和已脱敏报告，必要时提供可公开的最小结构样本，不提交真实 key 或数据库。

继续：[CLI](cli.md) · [工作区](workspace.md) · [配置](configuration.md)。
