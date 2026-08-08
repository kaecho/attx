# attx

**Agent Translation Toolkit eXtensible** — コーディングエージェントと人間のための、純 Rust・単一バイナリ・フォーマット非依存の AI 翻訳フレームワーク。

```
extract (format adapter) → translate (LLM core) → writeback (format adapter)
```

ゲーム（RPG Maker MV/MZ、Ren'Py、MTool）、電子書籍（EPUB）、文書（DOCX/XLSX/TXT/MD）、字幕（SRT/VTT/ASS/LRC）、ローカライズファイル（PO、i18next、Paratranz、VNTextPatch）を、**任意の OpenAI 互換 LLM** で翻訳できます。進捗は SQLite ワークスペースにキャッシュされるため、中断した実行は無料で再開できます。

## 他のツールと何が違うのか

- **エージェントファースト。** attx は stdout に JSON を出力するローカル CLI です — コーディングエージェントにとってネイティブなツール面です。Skill（`skills/attx/`）が実行プロトコルです：段階的パイプライン、ハードストップ、Q&A 設定ウィザード。MCP サーバーは不要です。
- **19 の組み込みアダプター** に加えて、**カスタムフォーマットプロファイル**（`line_regex` / `json_keys` / `json_paths` の TOML ルール）でその他のフォーマットにも対応。
- **設計上レジューム可能。** すべてのユニットが `attx.db` にチェックポイントされます。`translate` を再実行して続行；モデルに送信されるのは保留ユニットだけです。
- **正直な失敗。** モデルが失敗し続けるユニットは、目に見える *passthrough*（パススルー）プレースホルダーになります — 実行は完了し、`--retry-passthrough` でそれらのユニットだけを正確に再キューします。
- **自己改善。** 成功した実行は抽出経験を残し、何かを削除する前にあなたがレビューします。

## エージェントから始める（最速）

1. バイナリをインストール（[Releases](https://github.com/emptysuns/attx/releases) または `cargo build --release`）
2. Skill をインストール：`cp -a skills/attx ~/.claude/skills/`
3. エージェントに指示：

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
Help me set up attx if needed, then translate <input> from Japanese to Simplified Chinese.
```

Skill は、`setting.toml` がないとき **Q&A ウィザード**（endpoint、API key、model、言語）を実行し、キーをディスクにのみ書き込み、その後 `doctor --ping` → detect → extract → 試し翻訳 → 本番実行 の順に進めます。

## 自分でやる場合

```bash
cp setting.example.toml setting.toml   # base_url / api_key / model を記入
attx doctor --ping
attx run --input novel.epub --src ja --dst zh   # → novel.zh.epub
```

## 対応範囲

電子書籍、文書、字幕、ローカライズ JSON/PO、Ren'Py、RPG Maker、未知フォーマット用のカスタム TOML プロファイル — [Formats](formats.md)（フォーマット）を参照。

続き：[インストール](install.md) · [エージェント](agents.md) · [使い方](usage.md)
