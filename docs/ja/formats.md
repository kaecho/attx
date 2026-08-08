# フォーマット

| id | 入力 | 出力 |
|----|------|------|
| `rmmz` | ゲームフォルダ | その場 + `*.attxbak` |
| `epub` / `html` / `docx` / `xlsx` | ファイル | 訳出コピー |
| `txt` / `md` / 字幕 / `po` / `renpy` / `csv` | ファイル | 訳出コピー |
| `mtool` / `paratranz` / `vnt` / `i18next` | `.json` | 訳出コピー |
| `jsonl` / `custom:<name>` | 汎用 | プロファイル可 |

```bash
attx formats
attx detect --input <path>
```

未知形式は `analyze` → `profile new/test/save` を参照。
