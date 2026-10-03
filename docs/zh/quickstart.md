# 快速开始

这个示例把一本日文 EPUB 翻译为简体中文。其他格式使用同一入口，但输出方式可能不同，尤其是 RMMZ 的原地写回。

不想自己安装、编辑配置或记命令，可以改用[Agent 翻译向导](agent-translation.md)：复制一段提示词，Agent 阅读文档并逐步解释、询问、自动配置和翻译。以下是可选的手动命令路径。

## 1. 准备模型配置

先按[安装](install.md)获取 `attx`。复制发行包或仓库的示例配置，只在本机编辑密钥：

```bash
cp setting.example.toml setting.toml
```

最小配置如下。`YOUR_API_KEY` 是占位值，必须在本机替换，不能直接运行：

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
base_url = "https://api.example.com/v1"
api_key = "YOUR_API_KEY"
model = "provider-model-name"
```

端点应提供 `/chat/completions`；不要把这个后缀重复写进 `base_url`。省略其他小节会使用[配置默认值](configuration.md)。现有配置有效时无需重建。

## 2. 确认接口和输入

Linux/macOS（bash、zsh）：

```bash
attx --config './setting.toml' doctor --json --ping
attx detect --input './books/我的 小说.epub'
```

Windows PowerShell（假设尚未加入 PATH）：

```powershell
.\attx.exe --config '.\setting.toml' doctor --json --ping
.\attx.exe detect --input 'D:\Books\我的 小说.epub'
```

看 `doctor` 的 `llm.configured` 和 `ping` 字段，不能只看顶层 `status`。401/403、错误模型或端点应先修正，不能不停试连。`detect` 应报告 `engine: "epub"`。它不调用模型。

## 3. 正常全量翻译并写回

Linux/macOS：

```bash
attx --config './setting.toml' run --input './books/我的 小说.epub' --src ja --dst zh
```

Windows PowerShell：

```powershell
.\attx.exe --config '.\setting.toml' run --input 'D:\Books\我的 小说.epub' --src ja --dst zh
$LASTEXITCODE
```

不用在每个阶段重新确认。`run` 执行提取、翻译、机械审校、有限修复和写回。术语表构建默认关闭。普通 EPUB 输出为原文件旁的 `我的 小说.zh.epub`；实际路径以 `writeback.paths` 为准。默认工作区为文件父目录下 `.attx-我的 小说`。

如果输入路径含空格、括号或非 ASCII 字符，整个路径作为一个带引号参数传入。PowerShell 单引号不会展开 `$`；bash/zsh 单引号也保留 `$` 和反斜线。路径本身含单引号时改用符合当前 shell 规则的双引号和转义，不要复制其他 shell 的续行符。PowerShell 多行使用反引号，bash/zsh 多行使用反斜线；本页故意用单行避免混淆。

## 4. 读取结果

| 退出码 | 下一步 |
|--------|--------|
| 0 | 本次操作完成，检查输出路径和是否限制范围 |
| 1 | 致命错误，按 stderr 修正条件；不保证有完整 JSON |
| 2 | JSON 已给出未完成或被拦截结果，阅读 `status`、`review`、`skipped_note` |

`status="needs_attention"` 或 `blocked` 不是成功。`passthrough` 仍是原文，`pending=0` 也可能存在残留。中文清理和游戏排版不会代替人工语义审校。详细解释见[质量页](quality.md)。

## 可选：只做样本或预览

只有需要限制费用或先看样本时才运行：

```bash
attx run --input './books/我的 小说.epub' --src ja --dst zh --limit 20 --no-writeback --no-glossary --no-infer
attx translate --workspace './books/.attx-我的 小说' --dry-run
attx writeback --workspace './books/.attx-我的 小说' --dry-run
```

20 个单元不一定是 20 行，也不保证精确费用。`--dry-run` 不写缓存或输出，不发翻译请求。未全译时写回预览也可能报告 `blocked`。`run` 没有 `--dry-run`，不要给它添加这个参数。

## 中断或仍有问题

保留工作区，重跑同一条 `run` 复用已提交的有效译文。无需删除数据库：

```bash
attx status --workspace './books/.attx-我的 小说'
attx review --workspace './books/.attx-我的 小说'
attx repair --workspace './books/.attx-我的 小说'
attx writeback --workspace './books/.attx-我的 小说'
```

修复仍有限；达到上限后按[人工 JSONL 校对](usage.md)处理。只有明确接受部分文件时才使用 `writeback --allow-partial`，它保留未解决位置的原文并返回 2。

下一步：[日常用法](usage.md) · [完整 CLI](cli.md) · [让 agent 执行](agents.md)。
