# RPG Maker MV/MZ

エンジン ID：`rmmz`。入力は**ゲームディレクトリ**（`data/`、通常は `js/` も）。

## 手順

```bash
attx detect  --input /path/to/game
attx init    --input /path/to/game --src ja --dst zh
attx extract --workspace /path/to/game/.attx
attx translate --workspace /path/to/game/.attx
attx writeback --workspace /path/to/game/.attx --dry-run
attx writeback --workspace /path/to/game/.attx
```

書き戻しは**その場更新**で、変更ファイル横に `*.attxbak` を作ります。

## ドメイン

| domain | 内容 |
|--------|------|
| `dialogue` | メッセージ本文（`401`） |
| `namebox` | MZ ネームボックス `101` `parameters[4]`（0.7+） |
| `choices` / `scroll` | 選択肢 / スクロール |
| `system` / `base` | System・データベース |
| `plugins` | `js/plugins.js` のみ（プラグイン本体は触らない） |

`\N[n]` のネームボックスは抽出しません。

## 行リフロー（0.7+）

長い訳文を元の `401` スロット数に、表示幅（半角≈1、CJK≈2、制御≈0）で再配置します。`\C[n]` などは途中で分割しません。既定幅 **44**。

## プラグイン

- 多重 JSON 文字列パラメータは抽出と対称に decode / encode
- パラメータ名に `/` が含まれる場合も payload で正しく解決
- `key` / `FilePath` など ID・パス系はスキップ
