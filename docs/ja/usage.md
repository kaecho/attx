# 使い方

## ワンショット

```bash
attx run --input book.epub --src ja --dst zh
# → book.zh.epub が入力の隣に生成されます；元ファイルは絶対に触られません
```

`run` = `init` → `extract` →（任意で用語集）→ `translate` → `writeback` で、各段階の JSON レポートを出力します。役立つフラグ：

| フラグ | 効果 |
|------|--------|
| `--limit 20` | 試し実行：最大 20 ユニットを翻訳 |
| `--no-translate` | 抽出のみ行い、ステータスを出力 |
| `--no-writeback` | translate の後で停止（書き出す前に確認） |
| `--glossary` / `--no-glossary` | この実行の `[glossary].enabled` を上書き |

## ステップバイステップ（大きな入力向け）

```bash
attx detect  --input book.epub
attx init    --input book.epub --src ja --dst zh      # ワークスペース: .attx-book/
attx extract --workspace .attx-book
attx status  --workspace .attx-book
attx translate --workspace .attx-book --limit 20      # 先に試し実行
attx translate --workspace .attx-book                 # 本番；再実行でレジューム
attx writeback --workspace .attx-book --dry-run       # 予定されるファイルをプレビュー
attx writeback --workspace .attx-book                 # → book.zh.epub
```

### ワークスペース

ディレクトリ入力は `<dir>/.attx` に、ファイル入力は `<parent>/.attx-<stem>` になります。

| ファイル | 役割 |
|------|------|
| `attx.db` | SQLite：ユニット、翻訳、ワークスペースメタ |
| `workspace.json` | ワークスペースメタの可読スナップショット |
| `glossary.toml` | 用語集が構築されたときの用語 |
| `experience.toml` | このワークスペースの skip フィールド規則と `topic=prompt` 文体ノート |
| `profile.toml` | カスタムプロファイルが使われたときのそのコピー |

### `status` の読み方

```json
{
  "engine": "txt",
  "game_path": "/path/book.txt",
  "source_lang": "ja",
  "target_lang": "zh",
  "total": 1000,
  "translated": 20,
  "pending": 980,
  "passthrough": 0,
  "domains": { "text": { "total": 1000, "translated": 20 } }
}
```

- `pending` = 有効な翻訳がない（またはソースが変更された）ユニット。`translate` はこれらにのみ触れます。
- `passthrough` = モデルが拒否したか失敗し続けたため、「翻訳」が未加工の原文のままのユニット。実行を完了させるため翻訳済みとして数えられます — `translate --retry-passthrough` で再キューします。

## 出力の規約

- **ファイルフォーマットは翻訳済みの兄弟ファイル** `<name>.<dst>.<ext>` を書き出します；ソースファイルは決して変更されません。
- **`rmmz`（RPG Maker）はゲームディレクトリにインプレースで書き込みます**（`data/*.json`、`js/plugins.js`）。上書きする各ファイルは一度だけ `*.attxbak` にバックアップされます。
- **`jsonl` のディレクトリモード** は `source.jsonl` の隣に `translated.jsonl` を書き出します。
- `overwrite = true` のカスタムプロファイルもインプレースで書き込みます。

実行に何かを上書きさせる前に、必ず最初に `writeback --dry-run` を実行して `paths[]` を確認してください。

## 手動 / オフラインレビュー（JSONL）

保留ユニットをエクスポートし、手で編集（または人間がレビュー）し、インポートして書き戻します：

```bash
attx export-jsonl --workspace .attx-book --output pending.jsonl --filter pending
attx import-jsonl --workspace .attx-book --input pending.jsonl
attx writeback    --workspace .attx-book
```

フィルター：`pending`（デフォルト）| `all` | `translated` | `passthrough`。インポートはユニットを `id` で照合し、空でない `translation_lines` または `translation` を必要とします。

ワークスペースなしで単独実行：

```bash
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh
```

入力行：`{"id","text","context"?,"role"?,"item_type"?}` → 出力行には `translation` と `translation_lines` が追加されます。

## 未知の入力？ディスカバリツールチェーン

```bash
attx analyze --input ./project              # 偵察レポート（JSON）
attx profile new --output fmt.toml          # コメント付きルールテンプレート
attx profile test --profile fmt.toml --input ./project --roundtrip
attx init --input ./project --profile fmt.toml --src ja --dst zh
attx profile save --profile fmt.toml        # 以降 detect が認識します
```

完全なワークフロー：[Formats](formats.md)（フォーマット）→ *Unknown format? Teach attx a profile*（未知のフォーマット？attx にプロファイルを教える）。

## 用語集（任意）

作品全体で一貫した固有名詞。デフォルトではオフ — 構築には追加の LLM 呼び出しがかかります。

```bash
attx glossary build --workspace .attx-book --dry-run   # 規模を確認、費用はゼロ
attx glossary build --workspace .attx-book
attx glossary list  --workspace .attx-book
attx glossary check --workspace .attx-book             # 翻訳が無視した用語
```

README の *Glossary* セクションで、LLM 抽出戦略と `[glossary]` 設定キーを参照してください（`min_occurrences` のデフォルトは 10）。

## 経験 / 学習

writeback の後、attx はスキップ/抽出のヒントを自動的に記録します（デフォルトで API コストはかかりません）：

```bash
attx learn pending                       # 承認待ちのエントリ（証拠付き）
attx learn review --approve 1,3          # 承認 — これ以降にのみ何かを削除できます
attx learn list                          # アクティブなエントリ
attx writeback --workspace .attx --no-learn   # この 1 回だけキャプチャをスキップ
attx extract --no-knowledge              # 学習済みルールをすべて無視
```

## CLI マップ

| コマンド | 役割 |
|---------|------|
| `doctor [--ping]` | 設定 / LLM ピング |
| `formats` / `detect` / `analyze` | フォーマット |
| `profile …` | カスタムプロファイル |
| `init` / `extract` / `translate` / `writeback` / `run` | パイプライン |
| `status` / `export-jsonl` / `import-jsonl` | 進捗 & データ交換 |
| `learn …` / `glossary …` | 経験 & 用語 |

フラグ付きの完全なリファレンス：[CLI](cli.md)。
