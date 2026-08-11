# attx

**Agent Translation Toolkit eXtensible** —— 一个纯 Rust、单二进制、格式无关的 AI 翻译框架，面向编程 agent 与人类用户。

```
extract (format adapter) → translate (LLM core) → writeback (format adapter)
```

用**任意 OpenAI 兼容的 LLM** 翻译游戏（RPG Maker MV/MZ、Ren'Py、MTool）、电子书（EPUB）、文档（DOCX/XLSX/TXT/MD）、字幕（SRT/VTT/ASS/LRC）与本地化文件（PO、i18next、Paratranz、VNTextPatch）。进度缓存在 SQLite 工作区中，中断的运行可免费续跑。

## 它有何不同

- **Agent 优先。** attx 是一个本地 CLI，stdout 输出 JSON —— 编程 agent 的原生工具面。Skill（`skills/attx/`）就是执行协议：分阶段流水线、硬停止与 Q&A 配置向导。无需 MCP 服务器。
- **19 个内置适配器**，外加面向其他一切格式的**自定义格式 Profile**（`line_regex` / `json_keys` / `json_paths` TOML 规则）。
- **设计上可续跑。** 每个单元都在 `attx.db` 中打点存档。重跑 `translate` 即可继续；只有待译单元会发给模型。
- **诚实的失败。** 模型反复失败的单元会变成可见的 *passthrough* 占位 —— 运行照常完成，`--retry-passthrough` 会精确地重新入队这些单元。
- **自我改进。** 成功的运行会留下提取经验，在删除任何内容之前都会先由你审阅。

## 用 agent 开始（最快）

1. 安装二进制（[Releases](https://github.com/kaecho/attx/releases) 或 `cargo build --release`）
2. 安装 Skill：`cp -a skills/attx ~/.claude/skills/`
3. 告诉 agent：

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
Help me set up attx if needed, then translate <input> from Japanese to Simplified Chinese.
```

当 `setting.toml` 缺失时，Skill 会运行 **Q&A 向导**（端点、API Key、模型、语言），把 Key 只写到磁盘，然后运行 `doctor --ping` → detect → extract → 试译 → 全量运行。

## 或者自己动手

```bash
cp setting.example.toml setting.toml   # fill base_url / api_key / model
attx doctor --ping
attx run --input novel.epub --src ja --dst zh   # → novel.zh.epub
```

## 覆盖范围

电子书、文档、字幕、本地化 JSON/PO、Ren'Py、RPG Maker、面向未知格式的自定义 TOML Profile —— 见 [格式](formats.md)。

继续阅读：[安装](install.md) · [Agent](agents.md) · [用法](usage.md)
