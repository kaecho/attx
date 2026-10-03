# CLI 参考

```text
attx [--config <path>] [--client <name>] <command> [options]
```

全局 `--config` 选择配置文件，`--client` 选择其客户端。常规 `-h`/`--help` 查看帮助，顶层 `-V`/`--version` 查看版本。默认语言参数 `--src ja`、`--dst zh`；它们是字符串，不是只能接受 ja/en 的枚举。支持程度受源语言启发式和模型影响。

`--input` 的 `--game` 别名适用于 detect、analyze、init、run、profile infer/test；独立 translate-jsonl 没有这个别名。`learn scan` 是 `learn summarize` 的别名。

## 输出与退出码

正常命令输出 JSON 到 stdout，批次进度和诊断到 stderr。例外：doctor 默认人类可读，`learn defaults` 输出 TOML，help/version 输出文本。

| 退出码 | 含义 |
|--------|------|
| `0` | 本次命令完成；dry-run 或离线 review 不代表全量译文无误 |
| `1` | 执行时致命错误，stderr `error:`，不保证完整 JSON |
| `2` | 非预览翻译/修复/写回/run 未完成，仍输出结构化 JSON；也可能是 CLI 参数解析错误，此时看 stderr 区分 |

`translate`、`repair`、`translate-jsonl` 的未完成结果为 `needs_attention`；默认写回拦截为 `blocked`；显式部分写回为 `needs_attention`。dry-run 的非 ok 计划仍返回 0。`status`、`review`、`glossary check` 发现问题并不自动返回 2。doctor 的 `status="ok"` 也不替代实际配置/连接字段。

## 检查和检测

| 命令 | 参数与结果 |
|------|------------|
| `doctor` | `[--ping] [--json]`；JSON 含 `llm`（configured、可选 name/model/base_url/error）、`ping`、`adapters[]`、`saved_profiles[]`、`status`；不含密钥。ping 有一次小请求，失败可仍返回 0，必须检查字段。缺少示例配置时可在当前目录生成示例 |
| `formats` | 返回 `formats[]`，项含 id、label、extensions、input，保存 Profile 另有 profile |
| `detect` | `--input <path>`；返回 engine、content_root、label、profile，优先专用格式、保存 Profile、auto；不能传 --engine |
| `analyze` | `--input <path> [--src ja]`；返回 builtin_detect、saved_profile_detect、details、next_steps；文件含大小/编码/源文行密度/JSON结构/样本，目录含扩展名统计及 peek，二进制含容器提示 |

## Profile 子命令

| 命令 | 全部选项 | 主要报告 |
|------|----------|----------|
| `profile new` | `--output <path> [--name myformat]` | written、next、status；拒绝覆盖 |
| `profile infer` | `--input <path> --output <path> [--src ja] [--name auto-profile]` | profile、engine、units、attempts、output、roundtrip=true、overwrite=false、status；有模型费用，最多三次提案/验证 |
| `profile test` | `--profile <path或name> --input <path> [--src ja] [--limit 10] [--roundtrip]` | profile、engine、units、sample（location/role/text）、detects；可附 roundtrip 的 ok/output_files/outputs 或 error |
| `profile save` | `--profile <path> [--force]` | saved、status；保存到用户 Profile 目录 |
| `profile list` | 无 | profiles（name/engine/label/extensions/path）、dirs |

`test --limit` 限制报告样例，不限制实际提取；`--roundtrip` 只在内存中调用标记写回。语法及验证边界见[Profile 页](profiles.md)。

## 工作区流水线

```text
init --input <path> [--engine <id>] [--profile <path或name>] [--src ja] [--dst zh] [--workspace <dir>]
extract --workspace <dir> [--no-knowledge]
translate --workspace <dir> [--limit N] [--dry-run] [--retry-passthrough]
repair --workspace <dir> [--limit N] [--dry-run]
writeback --workspace <dir> [--dry-run] [--no-learn] [--allow-partial]
status --workspace <dir>
review --workspace <dir>
```

- init 创建/打开工作区，输出 workspace、status。保存 Profile 可用 `--engine custom:<name>`，显式 --profile 复制为工作区快照。默认目录为内容根 `.attx` 或文件父目录 `.attx-<stem>`。
- extract 输出 extracted、skipped_by_knowledge、rules_applied、status，auto 可附 auto_coverage。--no-knowledge 忽略所有经验过滤，仅用于诊断；不是禁用机器数据保护。
- translate 处理 pending 及已有被机械标记的单元，--retry-passthrough 先重新排队失败占位，--limit 限制本轮单元。初译后可自动有限修复。
- repair 按全量候选集合处理机械问题，--limit 限制候选，--dry-run 不发 HTTP 或改缓存。
- writeback 默认完整性门禁，--allow-partial 明确保留未解决原文，--no-learn 跳过本轮成功写回后的经验总结。
- status 输出 engine、game_path、source_lang、target_lang、total、translated、pending、passthrough、domains（按域 total/translated）。passthrough 不算成功。
- review 返回[质量页](quality.md)的 Report，不发 HTTP，问题本身不使此离线命令退出 2。

### 翻译与修复报告

| 字段 | 含义 |
|------|------|
| `pending_before` | 操作前待译量 |
| `translated` | 操作前 pending、最终通过的单元数量，包含后续修复成功；不是全工作区总量 |
| `pending_after` | 操作后仍 pending 数量 |
| `passthrough` | 最终仍保留的失败占位数量 |
| `dry_run` | 是否只有计划 |
| `skipped_note` | 跳过或未完成说明 |
| `status` | ok 或 needs_attention |
| `planned` | 本轮计划处理数量 |
| `repaired` | 操作前已有缓存但被标记、最终通过的单元数量；与本轮新完成的 translated 单元集合不重叠 |
| `repair_rounds` | 本次实际修复轮数 |
| `unresolved` | 最终唯一未解决候选数量 |
| `normalized_lines` | 中文规范化行数信号 |
| `review` | 完整机械审校报告 |

`translate-jsonl` 也用该结构，但没有工作区修复循环，repaired/repair_rounds 为 0。

### 写回报告

`files`、`units_applied`、`paths[]` 是实际写回规模，dry-run 时仅为计划。`dry_run`、`status`、`skipped_note`、`units_skipped` 说明完整性；`normalized_lines`、`reflowed_units`、`overflow_lines` 说明规范化/布局；`review` 为审校，运行经验总结时才附 `learned`。blocked 通常 files=0、paths=[]，不会生成输出或修改规范化缓存。

review 含 total、translated、pending、passthrough、glossary 和 residual_source/kana_edge/kana_mixed/kana_untranslated/identical/control_loss/namebox_mismatch。每类 count 加最多 40 个 `{location,unit_id,detail}` 样例。auto_coverage 含 supported_files、copied_files、unsupported_total、unsupported_paths、excluded_entries、excluded_paths；两个路径样例各最多50项，排除目录根计1。这些是报告样例上限，不是质量检查范围上限。

## run

```text
run --input <path> [--engine <id>] [--profile <path或name>] [--src ja] [--dst zh]
    [--workspace <dir>] [--limit N] [--no-translate] [--no-writeback]
    [--glossary | --no-glossary] [--no-infer] [--allow-partial]
```

按实际阶段返回 workspace、extracted、extract、可选 profile_inference/glossary/translate/writeback、review 和 status。--no-translate 不调用模型翻译、不写回，也不触发正常自动推断。--no-writeback 保留缓存；--no-infer 只禁止模型格式推断。run 没有 --dry-run 和 --no-learn。--glossary 与 --no-glossary 冲突。

未知输入的推断只在无法检测且允许翻译时执行，复用工作区规则。术语构建失败可以报告 glossary.error 并继续，但致命鉴权/配置错误停止整个任务。正常输入的完整翻译及写回不需要额外强制权限步骤。

## JSONL 交换

```text
translate-jsonl --input <path> --output <path> [--src ja] [--dst zh] [--limit N]
export-jsonl --workspace <dir> --output <path> [--filter pending|all|translated|passthrough]
import-jsonl --workspace <dir> --input <path>
```

export 默认 pending，返回 exported、output；import 返回 imported、status。记录至少有 id、text，可加 context、role、item_type，输出附 translation、translation_lines。独立输入中完全重复的 id+text 记录会警告并去重；这不同于审校导入，后者所有重复单元都拒绝。输入 ID 应由外部提取器保证唯一。

导入对整文件先验证再事务保存，未知 ID、原文变化、同单元别名重复和结构/保护错误拒绝整批。空译文跳过。独立命令无缓存续译和工作区修复循环，可能生成仍有空/占位记录的 JSONL 并退出 2；外部写回程序需阅读报告。详细流程见[用法](usage.md)。

## preserve 子命令

```text
preserve list --workspace <dir>
preserve add --workspace <dir> --pattern <regex> [--info <说明>]
preserve remove --workspace <dir> --pattern <regex>
```

list 返回 engine、count、rules（pattern/info/source）；add 返回 added、file、status；remove 返回 removed、pattern、status。空匹配和非法正则拒绝。只删除工作区中完全相同 pattern 的规则，不删除内置规则。保护结构见[工作区](workspace.md)。

## glossary 子命令

所有子命令都要求 `--workspace <dir>`。

| 子命令 | 其他选项 | 报告 |
|--------|----------|------|
| `build` | `[--min-occurrences N] [--dry-run]` | candidates、above_threshold、truncated、asked、added、rejected、total_active、min_occurrences、dry_run、file、可选sample；预览最多3批摘要，实际最多10术语样例 |
| `list` | `[--all]` | source_lang、target_lang、count、terms；--all 包含 rejected |
| `add` | `--src <term> --dst <translation> [--info <说明>] [--case-sensitive]` | added、file、status |
| `remove` | `--src <term>` | removed、src、status |
| `import` | `--file <json>` | imported、status |
| `export` | `--file <json>` | exported、file、status |
| `check` | 无 | active_terms、terms_seen、terms_fully_applied、violations（src/dst/occurrences/applied） |

build 有额外费用，即使配置 enabled=false，显式运行也会构建。最多 40 源文批次，不保证全文覆盖；truncated 只表示因 max_terms 截掉的术语候选数，不是源文覆盖报告。候选次数按包含该源文的单元数计算，同一多行单元只计一次。check 是子串建议，有违反项仍可返回 0；没有术语表时显式 check 报错，嵌入 review 则显示空统计。

## learn 子命令

```text
learn summarize --workspace <dir> [--llm]
learn note --text <要求> [--topic prompt] [--name <id>] [--workspace <dir> | --format <id>]
learn pending
learn review [--approve 1,3] [--reject 2] [--approve-all]
learn list [--format <id>] [--workspace <dir>]
learn defaults --format <id>
learn forget --field <字段> [--format <id>]
learn forget --name <id> [--workspace <dir> | --format <id>]
```

- summarize/scan 普通运行不调模型，--llm 有额外费用，返回 format、units_scanned、fields_seen、entries_written、pending、notes、file；写入全局格式经验。
- note 默认 topic=prompt，name 默认空，同名更新。作品用 workspace，全局用 format；其他 topic 仅供人读，不进入翻译 prompt。返回 format、topic、name、source、text、file、layer、reaches_prompt。至少选 workspace 或 format。
- pending 返回 pending、entries，每项有从 1 起的 index、format 和证据。review 按刚读取的索引批准/拒绝，返回 approved、rejected、remaining、files_written；CLI 有 --approve-all，但代理不得自行使用。至少指定一种操作。
- list 返回 formats、knowledge、note，项含 format/version/kinds/entries，作品层另有 layer。不会把内置基线混入这份全局/作品列表。
- defaults 输出内置 TOML，目前 rmmz 有基线。
- forget 必须恰好指定 field 或 name；field 在全局经验中，不能配 --workspace。返回 forgotten 及 field/name、status。

经验、术语和保护规则的完整落盘结构、层优先级及审批边界见[工作区页](workspace.md)。命令源码：[src/main.rs](https://github.com/kaecho/attx/blob/main/src/main.rs)，代理契约：[cli-command-contract.md](https://github.com/kaecho/attx/blob/main/skills/attx/references/cli-command-contract.md)。
