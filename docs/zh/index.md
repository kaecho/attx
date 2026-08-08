# attx

**Agent Translation Toolkit eXtensible** — 抽取 → 翻译（任意 OpenAI 兼容 LLM）→ 写回。

单个 Rust 二进制。SQLite 工作区。中断可续跑。

## 为什么用它

- **格式适配器**，不是一次性脚本：游戏、电子书、文档、字幕、本地化文件
- **面向 Agent**：stdout JSON + 自带 Skill 协议
- **安全默认**：RPG Maker 写 `*.attxbak`；文档写旁路 `*.<dst>.*`

## 30 秒上手

```bash
cp setting.example.toml setting.toml
attx doctor --ping
attx run --input novel.epub --src ja --dst zh
```

RPG Maker：

```bash
attx run --input /path/to/game --src ja --dst zh --no-writeback
attx writeback --workspace /path/to/game/.attx
```

## 0.7 新特性

| 领域 | 变化 |
|------|------|
| MZ 名牌 | `code 101` `parameters[4]` → `namebox` 域，可写回 |
| 对白行 | 按显示宽度折回原有 `401` 槽（CJK / 控制符安全） |
| plugins.js | 嵌套 JSON 字符串参数、参数名含 `/` 可正确写回 |

继续：[安装](install.md) · [用法](usage.md) · [RPG Maker](rmmz.md)
