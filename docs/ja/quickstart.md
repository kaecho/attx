# クイックスタート

この例は日本語を中国語へ翻訳します。`--src ja --dst zh` を目的の言語に置き換え、パスに空白があれば引用符で囲みます。API を使う権限と、入力を翻訳する権利を確認してください。

## 質問に答えてエージェントに任せる

[エージェント翻訳ガイド](agent-translation.md)のプロンプトをコーディングエージェントへ渡してください。エージェントがプロジェクトの文書を読み、導入と設定作成を行い、接続先、API キー、モデル、入力、言語、予算などを必要な分だけ質問します。各話題で目的と影響、推奨値を説明するので、ユーザーは回答するだけで進められます。

API キーは通常のチャットに貼らず、実際に利用できる安全な秘密入力か記録されない非表示端末で渡します。安全な入力能力がない環境では、エージェントが不足する機能を説明して止めます。TOML の手動編集はこの方式の必須手順ではありません。使える既存設定は再利用します。

以下は、自分で設定して実行したい場合の手順です。

## 手動で POSIX から EPUB を翻訳

```bash
cp setting.example.toml setting.toml
chmod 600 setting.toml
# エディターで base_url / api_key / model を設定
attx doctor --json --ping
attx detect --input "./novel.epub"
attx run --input "./novel.epub" --src ja --dst zh
```

既定ワークスペースは `./.attx-novel`、出力は `./novel.zh.epub` です。`run` は抽出、任意の語彙表構築、翻訳、有限修復、検証、書き戻しを実行します。通常の翻訳依頼に対して、試訳や writeback の再承認を必須にはしません。

## 手動で Windows PowerShell から実行

```powershell
Copy-Item .\setting.example.toml .\setting.toml
notepad .\setting.toml
.\attx.exe doctor --json --ping
.\attx.exe detect --input "C:\Books\novel.epub"
.\attx.exe run --input "C:\Books\novel.epub" --src ja --dst zh
$LASTEXITCODE
```

設定を実行場所と別に置く場合は明示します。

```powershell
.\attx.exe --config "C:\Config\attx\setting.toml" run --input "C:\Books\novel.epub" --src ja --dst zh
```

`--config` と `--client` はグローバルオプションです。秘密情報を含む設定全体をログへ表示しないでください。

## 結果を判定

stdout は結果の JSON、stderr は進捗とエラーです。正常完了は 0、致命的な操作エラーは 1、不完全な翻訳やブロックされた書き戻しは 2 です。2 の場合も stdout JSON を読みます。引数解析エラーも CLI が 2 を返すことがありますが、その場合は通常の結果 JSON とは区別します。

- `status: "ok"`: 依頼された操作が完了しました。意味の完全性を保証する値ではありません。
- `needs_attention`: pending、passthrough、残留原文などが残っています。
- `blocked`: 既定の完全性条件により出力していません。
- `writeback.paths`: 実際の出力先。`dry_run: true` なら計画のみです。

実行後に確認できます。

```bash
attx status --workspace "./.attx-novel"
attx review --workspace "./.attx-novel"
```

## 明示的に試訳する

ユーザーが少量の試訳を希望した場合は、出力を止めてキャッシュを確認します。

```bash
attx run --input novel.epub --src ja --dst zh --limit 20 --no-writeback
attx export-jsonl --workspace .attx-novel --output sample.jsonl --filter translated
```

この 20 は文字数や HTTP 回数ではなくユニット数です。全体に pending が残る場合、試訳の結果は終了 2 になり得ます。全量を続けるには同じワークスペースで実行します。

```bash
attx translate --workspace .attx-novel
attx writeback --workspace .attx-novel
```

## 費用を使わず計画を見る

```bash
attx init --input novel.epub --src ja --dst zh
attx extract --workspace .attx-novel
attx translate --workspace .attx-novel --dry-run
attx writeback --workspace .attx-novel --dry-run
```

翻訳 dry-run は HTTP を送らず、書き戻し dry-run はキャッシュや出力を更新しません。翻訳未完了なら、書き戻しの計画にもブロック理由が出ます。`run` 自体に `--dry-run` はありません。

## ゲームと未知形式

RPG Maker はディレクトリを入力します。

```bash
attx run --input "./Game" --src ja --dst zh
```

live の `data/*.json` と `js/plugins.js` にバックアップ付きで書き戻します。[RPG Maker](rmmz.md)で原文ソースの優先順と表示スロットを確認してください。

拡張子を認識できなくても `auto` が安全に本文を見つけられる場合があります。

```bash
attx analyze --input "./script.bundletext"
attx run --input "./script.bundletext" --src ja --dst zh
```

専用形式、保存プロファイル、`auto` でも検出できなければ `run` は有料の宣言型プロファイル推論を試みます。禁止するには `--no-infer` を指定します。二進数データや安全に位置を特定できない入力は[プロファイル](profiles.md)または[JSONL](workspace.md)経路へ進みます。

中断時はデータベースを削除せず同じコマンドを再実行します。失敗項目の扱いは[品質と修復](quality.md)、詳しい操作は[使い方](usage.md)にあります。
