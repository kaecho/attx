# 二次开发

开始前阅读[架构](architecture.md)，并沿用当前实现模式。仓库是 binary-only Rust 项目，edition 2024、rust-version 1.89。不要为新增适配器引入异步运行时、额外测试依赖或第二套翻译/写回管线。

## 本地开发

```bash
git clone https://github.com/kaecho/attx.git
cd attx
cargo build --release --locked
cargo test --locked
cargo run --release -- --help
cargo run --release -- doctor --json
```

这些是贡献者可运行的检查命令，不是本手册宣称已在你的环境执行的结果。测试尽量用内联文本、临时文件和本地假 HTTP 服务，避免真实模型和凭据。环境变量测试共享进程状态时应避免并发干扰；发布验证以单测试线程运行。

Cargo.lock 应随依赖和版本变化维护；凭据、真实翻译工作区、模型日志和私密测试输入不能提交。

## 添加适配器

优先确认现有 auto 或声明式 Profile 能否安全覆盖简单规则。真正需要结构解析、转义、容器或复杂定位时，新增专用适配器。

1. 在 `src/adapter/<name>.rs` 实现现有 FormatAdapter 的 id、label、extensions、input_kind、detect、extract、writeback。
2. 在 `src/adapter/mod.rs` 声明模块并按检测优先级登记。专用格式在 auto 之前；共用后缀应按具体结构优先，不用泛化检测抢占别的格式。
3. extract 只产出可翻译的人类文字，保留稳定 location、source_line_paths、角色、场景和所需 payload，遵循源语言过滤。不要把资源 ID 或控制串当正文。
4. writeback 仅返回 OutputFile 列表（path、bytes、可选 permissions），不写磁盘、不发网络、不自己重复实现备份。按源文件提供适用权限；共享 fileio 使用 cap-std 目录句柄，Unix temporary 初始为0600，完成的备份/输出继承源模式。缺少译文保持源文；无效/部分输出应使用权威原始源文，不能沿用先前 live 译文掩盖失败。
5. 需要特殊渲染约束时在格式内定义明确规则，由共享流水线调用；不能通过乱改通用 TextUnit 偷偷改变其他适配器行为。

参考 plaintext 的小型文本实现、csv 的字段转义、jsonkv 的内容检测、epub/docx/xlsx 的 ZIP 成员处理和 auto 的跨度/编码逻辑。示例代码必须以当前 trait 签名为准，不能照搬其他项目的实现。

### 适配器测试清单

- 检测正确格式，同时拒绝相似但不兼容的输入。
- 提取正文、跳过机器数据/资源语句/注释，源语言不符时不乱提取。
- 原文 no-op 往返，不改无关字段、换行、标签、命令数量和锚点。
- 译文引号、换行、控制符、数组行数和转义可写回。
- pending、passthrough 与显式部分输出保持原文。
- 输入内容变化、新增/删除文本、过期 source_hash 和规则摘要不符会被拒绝。
- 旧编码和无法表示的目标字符有明确行为，不以有损替换掩盖失败。
- 若处理 RMMZ，首槽说话人、两槽正文溢出和命令索引不漂移。

这些应加入已有模块的 `#[cfg(test)]` 范式，不为简单 fixture 新增框架或 test-only crates。

## 增加配置项

配置至少同时修改：

- `src/config.rs` 的 serde 字段、缺失键默认函数以及小节 Default。
- 实际消费者，不只把字段解析进结构但不使用。
- config.rs 的 example_toml 与根目录 setting.example.toml 两个模板。
- 配置默认/解析测试、受影响的行为测试。
- 三种语言 configuration 页、README 相关段落、Skill CLI 契约和 CHANGELOG。

不要新增隐式环境变量优先级或自动费用开关。省略可选 HTTP 字段应保持不发送，extra 的 messages 保护与最后合并优先级应一致。provider_type 目前只是元数据，若真的新增原生协议，必须完整实现并清楚迁移消费者，不能只改字段说明。

## 增加命令或选项

在 main.rs 的 clap 命令树新增参数，再接入 pipeline 或现有业务模块。用户、agent 和单命令 run 共享行为，不能为代理写另一套无限修复代码。

同时维护：help、输入别名、默认值、互斥条件、JSON 报告、exit 0/1/2 和 dry-run 语义。所有命令消费者包括 Skill、文档和自动化示例都必须更新。报告样例 cap 不应误伤全量候选；不能只为更好看的计数删除错误条目。

增加测试覆盖 CLI 解析、默认完整性拦截、部分写回、源文校验和错误传播。致命 HTTP 错误应在可选 glossary 等层立即传播，不能吞掉鉴权失败后继续发送。

## 扩展术语、保护或经验

schema 定义在 glossary.rs、preserve.rs、knowledge.rs。复用现有按作品和按格式的层，不再维护一份平行规则系统。新增永久字段应有默认值、序列化往返与迁移考虑。

knowledge apply 只过滤已提取单元，不产生新的正文。unknown entry 应保留而不执行，pending destructive skip 必须保持审批。中文规范化要有清楚中文证据，保护片段数量和相对顺序不变，不能任意删假名。

Profile 推断只允许三种声明式规则、最多三次验证、overwrite=false，不执行生成代码。测试 decoded-text no-op 只是结构证据，不要把它写成对任意语法和译文的证明。

## CI、打包与发布

[`ci.yml`](https://github.com/kaecho/attx/blob/main/.github/workflows/ci.yml) 在 Linux/Windows 构建、测试并 smoke CLI。[`docs.yml`](https://github.com/kaecho/attx/blob/main/.github/workflows/docs.yml) 在文档/导航/README 改动时 strict 构建并部署 Pages。

[`release.yml`](https://github.com/kaecho/attx/blob/main/.github/workflows/release.yml) 由 `v*` tag 或手动指定 tag 触发。先验证源码，运行 locked 测试、release build、版本与 tag 对应检查、CLI smoke 和 strict docs，再运行以下打包矩阵：

| 目标 | 包 |
|------|----|
| x86_64-unknown-linux-gnu | attx-linux-x86_64.tar.gz |
| x86_64-pc-windows-msvc | attx-windows-x86_64.zip |
| aarch64-apple-darwin | attx-macos-aarch64.tar.gz |
| x86_64-apple-darwin | attx-macos-x86_64.tar.gz |

包包含二进制、英文/中文 README、CHANGELOG、LICENSE、示例配置、skills、profiles、docs、mkdocs.yml 和 requirements-docs.txt。不包含个人 setting.toml、数据库、真实输入或站点缓存。发布流程等所有包完成后上传 GitHub Release。

正式发布需核对 Cargo.toml 与 Cargo.lock 版本、CHANGELOG、README 和文档一致，验证源文/恢复风险，提交后创建对应 `v0.10.1` tag。CI 文件描述的是应执行的流程，不证明某个具体 Release 已发布或全部 runner 已成功。

## 文档与 Skill 贡献

文档源位于 docs/en、docs/zh、docs/ja，共用同名15页，导航在 mkdocs.yml。链接尽量同语言相对路径，引用源码用稳定 GitHub URL，不用开发者本机绝对路径。不要直接编辑生成的 site/。

```bash
python -m venv .venv-docs
# Linux/macOS
. .venv-docs/bin/activate
# Windows PowerShell 对应 .\.venv-docs\Scripts\Activate.ps1
pip install -r requirements-docs.txt
mkdocs build --strict
mkdocs serve
```

同步 README 和 skills/attx/references，避免同一参数存在不同默认值。prose 描述实际行为与边界，不写“一次完美”、免费的中断保证或 mandatory writeback permission。示例使用占位密钥且注明需本机填写；术语构建和 LLM 经验复核默认 opt-in。

## 来源与许可边界

设计说明可以注明参考 LinguaGacha v0.125.0 的失败单元重试、严格编号和行数检查、有限上下文及共享 agent/batch 思路。实现为独立 Rust 代码，不直接复制上游源码，不声称上游商业授权或许可兼容。attx 自身许可见仓库 LICENSE，依赖许可需按其自身条款处理。

继续：[架构](architecture.md) · [配置](configuration.md) · [CLI](cli.md)。
