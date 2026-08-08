# attx

**Agent Translation Toolkit eXtensible** — 抽取 → 翻译（任意 OpenAI 兼容 LLM）→ 写回。

单个 Rust 二进制。格式无关。SQLite 工作区，中断可续跑。

## 用 Agent 最快上手

1. 安装二进制（[Releases](https://github.com/emptysuns/attx/releases) 或 `cargo build --release`）
2. 安装 Skill：`cp -a skills/attx ~/.claude/skills/`
3. 对 Agent 说：

```text
严格遵循 <attx目录>/skills/attx/SKILL.md
帮我配置 attx（如需要），再把 <输入> 从日文翻译成简体中文。
```

Skill 在缺少 `setting.toml` 时走**问答向导**（端点、Key、模型、语向），Key 只写入磁盘，然后 `doctor --ping` → detect → 试译 → 全量。

## 或自己配置

```bash
cp setting.example.toml setting.toml
attx doctor --ping
attx run --input novel.epub --src ja --dst zh
```

## 覆盖范围

电子书、文档、字幕、本地化 JSON/PO、Ren'Py、RPG Maker、未知格式的自定义 Profile——见 [格式](formats.md)。

继续：[安装](install.md) · [Agent](agents.md) · [用法](usage.md)
