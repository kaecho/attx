# attx

**Agent Translation Toolkit eXtensible** — 抽出 → 翻訳（OpenAI 互換 LLM）→ 書き戻し。

Rust の単一バイナリ。SQLite ワークスペース。中断しても無料で再開できます。

## 特徴

- **フォーマットアダプタ**（ゲーム / 電子書籍 / 文書 / 字幕 / ローカライズ）
- **エージェント向け**：stdout は JSON、Skill プロトコル同梱
- **安全な既定**：RPG Maker は `*.attxbak`、文書は `*.<dst>.*` のコピー

## 30 秒

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

## 0.7 の新機能

| 領域 | 内容 |
|------|------|
| MZ ネームボックス | `code 101` `parameters[4]` を `namebox` として抽出・書き戻し |
| メッセージ行 | 表示幅で元の `401` スロットへリフロー（CJK / 制御コード安全） |
| plugins.js | ネストした JSON 文字列・`/` を含むパラメータ名の書き戻し修正 |

続き：[インストール](install.md) · [使い方](usage.md) · [RPG Maker](rmmz.md)
