# attx 中文手册

attx（Agent Translation Toolkit eXtensible）是一个 Rust 命令行翻译工具。它把格式处理、模型翻译和写回分开，用 SQLite 保存工作区进度。人可以直接运行命令，编程 agent 也可以按仓库中的 Skill 自动执行。

本手册对应 0.10.0。运行模型需要 OpenAI 兼容的 Chat Completions 服务；格式检测、提取、状态查询和机械审校可以离线完成。它不是游戏运行时补丁，也不承诺一次得到没有漏译、语义错误或排版问题的成品。

```text
输入 → 适配器提取 → 工作区缓存 → 模型翻译与有限修复 → 校验并写回
```

## 从哪里开始

| 需要 | 阅读 |
|------|------|
| 安装二进制或编译源码 | [安装](install.md) |
| 不读配置手册，复制提示词让 Agent 提问、安装与翻译 | [Agent 翻译向导](agent-translation.md) |
| 第一次完成翻译 | [快速开始](quickstart.md) |
| 配置模型、并发、费用和修复次数 | [配置参考](configuration.md) |
| 分阶段执行、断点续译、人工校对 | [日常用法](usage.md) |
| 确认格式、编码和输出范围 | [格式与未知输入](formats.md) |
| 定义或推断新格式 | [自定义 Profile](profiles.md) |
| 翻译 RPG Maker 游戏 | [RPG Maker MV/MZ](rmmz.md) |
| 理解残留、姓名框和溢出报告 | [质量与审校](quality.md) |
| 查命令、默认参数和 JSON 字段 | [CLI 参考](cli.md) |
| 让 agent 无人值守完成任务 | [Agent 自动化](agents.md) |
| 管理缓存、术语、保护规则和经验 | [工作区与持久化数据](workspace.md) |
| 理解源码的职责边界 | [架构](architecture.md) |
| 扩展适配器、配置、CLI，提交文档 | [二次开发](development.md) |
| 处理接口、源文、格式或写回失败 | [故障排查](troubleshooting.md) |

## 覆盖范围

专用适配器处理 RPG Maker MV/MZ、Ren'Py 翻译脚本、EPUB、HTML、DOCX、XLSX、文本、Markdown、字幕、CSV/TSV、PO 及几种本地化 JSON。`auto` 按内容保守识别 JSON、XML、简单标量配置和正文。声明式 TOML Profile 可补充已知的行或 JSON 结构，JSONL 可连接外部提取器。

目录被 `auto` 接管时，不支持的文件可能原样复制到输出。复制数量不是翻译数量，详细边界见[格式页](formats.md)。

## 怎样判断完成

看最终 stdout JSON 和退出码，不看 stderr 的批次进度。`pending=0` 不能证明全译完，`passthrough` 是保留原文的失败占位。模型翻译和自动修复都有限；剩余问题会进入报告。

默认写回拦截未完成或无效单元。用户明确接受部分结果时才使用 `--allow-partial`，原文会保留在未解决位置，退出码仍为 2。正常翻译请求已经包含该输入的正常写回，不另设强制许可步骤。原地写回有备份，输出先暂存再逐文件替换，但不是整个目录的事务。

## 设计来源

项目参考了 LinguaGacha 的失败单元重试、编号与遗漏检查、有限上下文、共享批次流程等思路，并独立用 Rust 实现。这里不主张复制其 TypeScript/Electron 源码，也不对上游授权或许可兼容性作推断。attx 的依赖和许可信息见[仓库](https://github.com/kaecho/attx)及[开发说明](development.md)。
