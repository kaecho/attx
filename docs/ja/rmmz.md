# RPG Maker MV/MZ

`rmmz` はゲームディレクトリを入力します。入力自身、`www/`、`game/` から `data/` と System.json または js directory を持つ content root を探します。MV と MZ のデータ形式を扱いますが、独自 binary やプラグインの全 runtime semantics を解析するわけではありません。

## 通常実行と出力

```bash
attx detect --input ./Game
attx run --input ./Game --src ja --dst zh
attx status --workspace ./Game/.attx
attx review --workspace ./Game/.attx
```

ワークスペースは検出した content root の `.attx` です。例えば root が `Game/www` なら `Game/www/.attx` です。`detect.content_root` と `init.workspace` の実値を使ってください。

writeback は live の `data/*.json` と `js/plugins.js` をゲーム内で更新します。プラグインソース `js/plugins/*.js` は変更しません。既存ファイルには拡張子を置き換えず `.attxbak` を付加し、最初のバックアップを保持します。通常翻訳を依頼された入力にはこの書き戻しが含まれ、追加許可を必須にはしません。

任意でプレビューできます。

```bash
attx writeback --workspace ./Game/.attx --dry-run
```

dry-run は出力も cache も変更しません。実際の書き戻しは全 artifact を準備してから各ファイルを置換します。backup 失敗は停止しますが、ディレクトリ全体の原子トランザクションではありません。重要なゲームには別の完全コピーも保持してください。

## 原文ソースを維持する

再抽出の原文は `data_origin` を優先し、次に `.attxbak` の原文を使います。翻訳済み live file を新しい原文として送信し直すのを避けるためです。出力先は `data_origin` ではなく live data です。

source hash、anchor、抽出集合の snapshot が workspace に保存され、writeback は原文の変更だけでなく本文の追加・削除も検査します。ゲーム更新などで原文が変わったら旧訳を無理に流し込まないでください。cache を削除して隠すのではなく、原文ソースを確認し、再抽出や新しい workspace で処理します。

## 抽出ドメイン

| domain | 対象 |
|---|---|
| `dialogue` | 文章の表示、event code 401 の行列 |
| `namebox` | MZ の code 101 の parameters[4] にある直接指定話者 |
| `choices` | code 102 の選択肢 |
| `scroll` | code 405 のスクロール本文 |
| `system` | System.json の用語、メッセージ、menu |
| `base` | Actors、Skills、Items などの名前、プロフィール、説明 |
| `plugins` | js/plugins.js の表示用 parameter 値と安全に decode できる nested JSON |

resource path、ID、switch、symbol、ファイル名、論理 handle を本文として訳すと動作を壊し得ます。アダプターの機械 literal 判定と形式別の経験ルールで避けます。`\N[n]` は control として保護し、直接話者名と同じように抽出しません。

## プラグインのパラメータ

`plugins.js` の parameter が JSON を文字列に入れた構造でも、対応する nested 層を decode して表示文字列を抽出し、writeback 時に encode し直します。parameter 名に `/` が含まれるケースにも anchor を用意します。

表示文言か論理名かは name heuristic だけでは確定できません。組み込み経験を見るには:

```bash
attx learn defaults --format rmmz
attx learn list --format rmmz
```

不適切な学習 filter と確認できた場合のみ `extract --workspace ... --no-knowledge` で比較します。学習は既に抽出されたユニットを除外するルールで、任意の plugin source から新しい本文を作る機能ではありません。

## 固定会話スロットと話者

会話の writeback は元の 401 スロット数とイベントコマンド数を変えません。訳文が長くなっても command を足したり削除したりせず、表示幅と句読点を考慮して元スロットへ再配置します。control token は分断しません。

先頭スロットが話者 label である場合、そのスロットを保護して本文を残りへ配置します。話者と本文を混ぜて先頭を消費する方法ではありません。2 スロットが label と body の場合、body は残り 1 スロットに収める必要があります。本文を黙って切り捨てず、避けられない残りは最後のスロットに保持します。

基準幅は 44 halfwidth cells です。最終スロットなどで収まりきらなければ `overflow_lines` に報告します。半角セルの推定は font、plugin、window size の実際の見え方を保証しません。`reflowed_units` は再配置したユニット数です。

Chinese 正規化と RMMZ 再配置は実際に成功した writeback で cache に同期します。後続の `export-jsonl` と `review` は同じ訳を見るため、ゲームファイルだけを手編集しないでください。修正には JSONL import を使います。

再度の `--allow-partial` 書き戻しでも、無効ユニットは抽出原文に戻します。以前 live に書かれていた旧訳を、その項目の現訳の代わりに残すことはありません。

## プレイテストと復旧

書き戻し後は、タイトル・menu・選択肢・話者框・長い会話・plugin の表示をゲームで確認します。機械レビューの ok は engine 起動と画面表示の合格を意味しません。

`.attxbak` はファイル単位の最初の原文で、実行ごとの history ではありません。復元、workspace リセット、ゲーム更新への cache 移行は別の変更です。対象を確認してから行い、エージェントは翻訳依頼だけを根拠に削除や全リセットをしません。

関連: [品質と修復](quality.md)、[ワークスペース](workspace.md)、[エージェント](agents.md)。
