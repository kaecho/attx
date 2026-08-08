# attx

[English](README.md) | **中文** | [文档](https://emptysuns.github.io/attx/zh/)

**Agent Translation Toolkit eXtensible** — 单个 Rust 二进制：抽取文本 → 任意 OpenAI 兼容 LLM 翻译 → 写回原格式。

```text
extract → translate → writeback
```

进度存在 SQLite 工作区，中断后可免费续跑。

## 安装

- **发行包：** [Releases](https://github.com/emptysuns/attx/releases)（`v*` 标签）
- **源码：**

```bash
cargo install --path .
# 或
cargo build --release && ./target/release/attx --help
```

## 快速开始

```bash
cp setting.example.toml setting.toml   # 填 base_url / api_key / model
attx doctor --ping

# 一键（电子书/文档/字幕 → 旁路生成 *.<dst>.*，不改原文件）
attx run --input novel.epub --src ja --dst zh

# RPG Maker MV/MZ（原地写回 + *.attxbak）
attx run --input /path/to/game --src ja --dst zh --no-writeback
attx writeback --workspace /path/to/game/.attx --dry-run
attx writeback --workspace /path/to/game/.attx
```

分步（大项目建议先试译 20 条）：

```bash
attx detect  --input <path>
attx init    --input <path> --src ja --dst zh
attx extract --workspace .attx-<name>
attx status  --workspace .attx-<name>
attx translate --workspace .attx-<name> --limit 20
attx translate --workspace .attx-<name>
attx writeback --workspace .attx-<name>
```

## 支持格式

| id | 输入 | 说明 |
|----|------|------|
| `rmmz` | 游戏目录 | MV/MZ `data/*` + `js/plugins.js`（**不改**插件源码） |
| `epub` / `html` / `docx` / `xlsx` | 文件 | 保留版式 |
| `txt` / `md` | 文件 | 按行/块 |
| `srt` / `vtt` / `ass` / `lrc` | 文件 | 保留时间轴 |
| `po` / `renpy` / `csv` | 文件 | gettext / Ren'Py / 表格 |
| `mtool` / `paratranz` / `vnt` / `i18next` | `.json` | 内容嗅探 |
| `jsonl` / `custom:<name>` | 文件/目录 | 通用出口 + TOML 自定义 profile |

`attx formats` 可列出全部（含已保存 profile）。

## RPG Maker 要点（0.7+）

- **名牌**（`code 101` / `parameters[4]`）进入 `namebox` 域并可写回
- **对白重排** — 超长中文按显示宽度折回原有 `401` 行槽，避免 UI 裁切
- **插件写回** — 多层 JSON 字符串参数、参数名含 `/` 均可正确 round-trip

## Agent Skill

```bash
cp -a skills/attx ~/.claude/skills/
# 其他 Agent：严格遵循 <attx>/skills/attx/SKILL.md
```

## 文档

完整说明（中 / 英 / 日）：**https://emptysuns.github.io/attx/zh/**

## 许可

MIT
