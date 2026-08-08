# 命令行

| 命令 | 作用 |
|------|------|
| `doctor [--ping]` | 配置 / LLM 探测 |
| `formats` | 适配器列表（JSON） |
| `detect --input` | 格式探测 |
| `analyze --input` | 未知输入侦察 |
| `profile new/test/save/list` | 自定义 profile |
| `init --input --src --dst` | 工作区 + SQLite |
| `extract --workspace` | 抽取 unit |
| `translate --workspace [--limit]` | 翻译 pending |
| `writeback --workspace [--dry-run] [--no-learn]` | 写回 |
| `run --input …` | 一键流水线 |
| `status --workspace` | 进度与分域 |
| `export-jsonl` / `import-jsonl` / `translate-jsonl` | 交换 |
| `learn …` / `glossary …` | 经验 / 术语 |

全局：`--config`、`--client <name>`。
