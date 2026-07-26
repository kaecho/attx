# 未知格式：分析 → 写 Profile → 试跑 → 记住

`attx detect` 报 "no format adapter matched" 时按本文流程走。目标：让 attx 学会这个格式，
下次同类文件**自动识别**。

## 决策树

```text
detect 失败
 └─ attx analyze --input <输入>
     ├─ binary=true / container=zip → 解包后对内部文件再 analyze；纯二进制走 JSONL 逃生舱
     ├─ 文本 / JSON → 写自定义 Profile（本文主流程）
     └─ 结构太复杂（如带偏移表的封包） → 外部提取脚本 + translate-jsonl
```

## 主流程（agent 按序执行）

### 1. 侦察

```bash
attx analyze --input <文件或目录>
```

读 JSON 报告：`encoding`（Shift_JIS/GBK 也能直接处理，无需 iconv）、
`sample_head`（判断行结构）、`json`（top_keys / 数组首元素 → 判断哪些字段是正文）、
目录时 `extensions` + `peek`。

### 2. 起草 Profile

```bash
attx profile new --output ./fmt.toml --name <格式名>
```

模板自带注释。规则三选一可混用（JSON 规则在文件可解析为 JSON 时生效）：

| kind | 适用 | 关键字段 |
|------|------|----------|
| `line_regex` | 行结构文本（脚本、ini、日志式） | `pattern`，命名组 `(?P<text>…)` 必须、`(?P<role>…)` 可选 |
| `json_keys` | JSON，正文字段名固定（任意深度） | `keys = ["message", …]`（值为字符串或字符串数组） |
| `json_paths` | JSON，按位置精确指定 | `paths`，`*` 一层 / `**` 任意层，如 `events/*/text` |

其他字段：`extensions`（目录扫描必填）、`skip_lines`（注释/命令行）、
`detect_regex`（自动识别辅助）、`overwrite`（true=原地写回并留 `*.attxbak`；默认写
`<名>.<语言>.<扩展名>` 副本）、`notes`（写清规则依据，给下个读者）。

参考样例：仓库 `profiles/examples/`（kirikiri-kag / ini-lang / json-messages）。

### 3. 迭代验证（不写盘）

```bash
attx profile test --profile ./fmt.toml --input <输入> --roundtrip
```

检查 JSON 输出：

- `units` 数量符合预期？太少 → 规则漏配；太多 → 把命令/路径行加进 `skip_lines`
- `sample[].text` 干净吗？不能混进标签、时间轴、变量名
- `sample[].role` 取到说话人了吗（有则填）
- `roundtrip.ok == true`（内存写回成功，不落盘）
- `detects == true`（否则补 `detect_regex` / 调低 `min_units`）

修改 → 重跑，直到三项都对。**禁止**为凑数把过滤放宽到会译坏代码/路径。

### 4. 正式翻译

```bash
attx init --input <输入> --profile ./fmt.toml --src ja --dst zh
attx extract --workspace <工作区>
attx status --workspace <工作区>            # 报告规模，>2000 条先问用户
attx translate --workspace <工作区> --limit 20   # 试译
attx translate --workspace <工作区>
attx writeback --workspace <工作区> --dry-run    # overwrite=true 时需用户许可再真写
```

Profile 会被拷贝进 `<工作区>/profile.toml`，工作区自包含、可复现。

### 5. 记住格式（问用户）

翻译成功后**问用户**："是否保存此格式 Profile，今后自动识别同类文件？" 同意则：

```bash
attx profile save --profile ./fmt.toml      # --force 覆盖同名
attx profile list                           # 确认
```

保存位置：`$ATTX_HOME/profiles/` 或 `~/.config/attx/profiles/`。
此后 `attx detect` / `attx formats` / `attx init --engine custom:<名>` 都认得它。

## 硬性边界

- Profile 是**声明式规则**，不是代码；不要试图用它处理二进制、加密封包 —— 那类走
  外部提取器 + `translate-jsonl`（见 `jsonl-workflow.md`）。
- `overwrite = true` 的 Profile 写回等同 rmmz：**先 dry-run，得到用户许可再写**。
- 写回输出一律 UTF-8。原文件是 Shift-JIS 且引擎只认 Shift-JIS 时，提醒用户可能需要
  转码回去或给引擎打 UTF-8 补丁（如 KiriKiri 加 BOM / 引擎设置）。
- 同一目录混多种结构时，可拆多个 Profile 分别 init 到不同 workspace。
