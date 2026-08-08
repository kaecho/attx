# RPG Maker MV/MZ

attx は**汎用**の翻訳フレームワークです — このページは RMMZ 固有の注意点のみです。主要な流れは [Usage](usage.md)（使い方）と [Agents](agents.md)（エージェント）を参照してください。

`rmmz` アダプターは**ゲームディレクトリ**を受け取ります。コンテンツルート `./`、`www/`、`game/` を検出します（`data/` に加えて `System.json` または `js/` が必要）。

## パイプライン

```bash
attx detect  --input /path/to/game
attx init    --input /path/to/game --src ja --dst zh      # ワークスペース: /path/to/game/.attx
attx extract --workspace /path/to/game/.attx
attx translate --workspace /path/to/game/.attx
attx writeback --workspace /path/to/game/.attx --dry-run  # paths[] をプレビュー
attx writeback --workspace /path/to/game/.attx
```

**Writeback はインプレースです**：`data/*.json` と `js/plugins.js` はゲームディレクトリ内で書き換えられ、上書きされる各ファイルは一度だけ `*.attxbak` にバックアップされます。常にドライランで確認してから実行してください — 実際の writeback の前にはエージェントはユーザーに確認を取らなければなりません。

## 抽出されるもの

| ドメイン | ソース |
|--------|--------|
| `dialogue` | 文章の表示 イベントコマンド（`401`） |
| `namebox` | MZ の話者プレート — イベントコマンド `101` の `parameters[4]` |
| `choices` | 選択肢の表示 イベントコマンド（`102`） |
| `scroll` | スクロール文章の表示 イベントコマンド（`405`） |
| `system` | `System.json` — 用語、メッセージ、メニュー |
| `base` | データベースの名前 / プロフィール / 説明（`Actors.json`、`Skills.json`、…） |
| `plugins` | `js/plugins.js` — プラグインの `@param` 値のみ、**プラグインのソースファイルは決して対象外** |

`\N[n]` ネームボックス参照は抽出されません。他の場所から参照されている `data/*.json` 内の名前は、学習済みルールによってスキップされます（組み込み経験の `plugins` ドメインを参照：`attx learn defaults --format rmmz`）。

## プラグインパラメータ

- 触られるのは `js/plugins.js` だけです — `.js` のプラグインソースは決して変更されません。
- 値がネストされた JSON 文字列であるパラメータは、抽出時にデコードされ writeback 時に再エンコードされるため、ネスト構造はラウンドトリップします。
- `/` を含むパラメータ名もラウンドトリップします。
- メッセージ行は表示幅に基づいて元の `401` スロット数にリフローされます。

## writeback の後

実行から学習された経験は次のプロジェクトに自動的に適用されます（README の *Self-improving experience layer* を参照）。`*.attxbak` ファイルで翻訳前の状態を復元できます；これらは gitignore されています。
