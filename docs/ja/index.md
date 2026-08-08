# attx

**Agent Translation Toolkit eXtensible** — 抽出 → 翻訳（OpenAI 互換 LLM）→ 書き戻し。

単一バイナリ。フォーマット非依存。SQLite で中断再開。

## エージェントで最速セットアップ

1. バイナリを入れる
2. Skill を入れる：`cp -a skills/attx ~/.claude/skills/`
3. 指示する：

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
Help me set up attx if needed, then translate <input>.
```

設定が無いときは **Q&A ウィザード**（endpoint / key / model / 言語）が走ります。

## 自分で設定

```bash
cp setting.example.toml setting.toml
attx doctor --ping
attx run --input novel.epub --src ja --dst zh
```

続き：[インストール](install.md) · [エージェント](agents.md) · [使い方](usage.md)
