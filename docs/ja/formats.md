# フォーマットと対応限界

`attx formats` が実行中バイナリの一覧を返します。0.10.0 は専用形式 19 種に `auto` を加え、宣言型の `custom:<name>` も使えます。検出は専用アダプター、保存済みプロファイル、`auto` の順です。`.json` は拡張子だけで決めず内容の形を調べます。

## 全組み込みアダプター

通常のファイル出力は `<stem>.<dst>.<ext>` です。既に出力先が存在すれば `.attxbak` を付加したバックアップを残します。入力の原文を保持する出力方式でも、既存の翻訳出力は更新され得ます。

| ID | 入力 | 抽出対象と限界 |
|---|---|---|
| `rmmz` | RPG Maker MV/MZ ディレクトリ | data の会話・選択肢・システム・DB、plugins.js の表示用パラメータ。ゲーム内に書き戻し。プラグインソースや任意の JavaScript は翻訳しない |
| `epub` | `.epub` | ZIP 内 XHTML の leaf block 段落、見出し、リストなど。ruby の rt/rp 読みを原文から除外。画像・非本文を保持し dc:language を更新。任意の CSS 表示結果まで保証しない |
| `html` | `.html`, `.htm`, `.xhtml` | EPUB と共通の本文抽出。script/style を本文扱いしない。ブラウザー実行やサイト全体のクロールではない |
| `docx` | `.docx` | 本文・脚注・文末脚注の w:t 段落。訳は最初のランへ集約するため、段落内の細かいランごとの文字装飾は均等に維持する保証がない |
| `xlsx` | `.xlsx`, `.xlsm` | `xl/sharedStrings.xml` の共有文字列。rPh 発音ランを除外。数式、画像、すべての inline string を翻訳する方式ではない |
| `srt` | `.srt` | cue 本文のみ。番号と時刻を保持。読み速度や画面内改行の自動最適化はしない |
| `vtt` | `.vtt` | WebVTT cue 本文。header、timestamp、metadata を保持。字幕 renderer の表示保証はない |
| `ass` | `.ass`, `.ssa` | Dialogue の Text フィールド。Name を role に利用。override tag と改行 `\N` を保持。スタイル表は訳さない |
| `lrc` | `.lrc` | タイムスタンプ付き歌詞。タイミング・メタデータは保持。音節同期の再調整はしない |
| `csv` | `.csv`, `.tsv` | 原文言語を含むセル。引用フィールド・埋め込み改行に対応。変更レコードは再レンダリングされるため、完全な元バイト配置を保証しない |
| `po` | `.po`, `.pot` | msgid に対する msgstr。header と msgid_plural 項目は対象外。複数形ルールを自動実装しない |
| `renpy` | `.rpy` | translate block 内の会話と old/new ペア。voice/play/show などの asset 文はスキップ。一般 Python コードやすべての未抽出脚本を翻訳しない |
| `md` | `.md`, `.markdown` | Markdown 本文。コード領域などを避ける。複雑な拡張 Markdown のあらゆる文法を完全解析する方式ではない |
| `txt` | `.txt` | 原文言語の行・本文。文章以外の機械データに強制しない |
| `paratranz` | 形を認識した `.json` | Paratranz の text/translation 項目。空の translation を埋める。既存訳を無条件に置き換える用途ではない |
| `vnt` | 形を認識した `.json` | VNTextPatch の name/message。専用 export 形が前提 |
| `mtool` | 形を認識した `.json` | MTool の原文から訳へのマップ、ManualTransFile.json など。任意の JSON オブジェクトを同じ意味で扱わない |
| `i18next` | ネストした `.json` | 文字列 leaf。キー、数値などは保持。変数を保護するが文法ごとの全複数形 semantics は検証しない |
| `jsonl` | `.jsonl` または source.jsonl のあるディレクトリ | 明示的な id/text レコード。原文言語フィルタなし。ディレクトリ出力 translated.jsonl、ファイル出力言語 suffix の副本 |
| `auto` | 内容を認識できるファイルまたはディレクトリ | 下記の安全なテキスト部分集合。元 encoding と BOM を保持。ディレクトリ出力 translated-<dst>/ |

専用アダプターのソースは [src/adapter](https://github.com/kaecho/attx/tree/main/src/adapter) にあります。強制する場合は `init --engine <id>` または `run --engine <id>` を使います。`detect` に `--engine` はありません。強制しても不正な構造を修復したり、binary を扱えるようにはなりません。

## `auto` の無損失テキスト処理

`auto` は拡張子に依存せず内容を認識し、元テキストの置換対象 span を記録します。対応 subset では翻訳しない syntax、空白、コメントをそのまま残し、翻訳値を適切に escape して構造を再検査します。JSON 全体を整形し直す方法とは異なります。

| 内容 | 対応範囲 | 対象外 |
|---|---|---|
| JSON | 任意の正しい JSON の表示用文字列値。キー、機械 literal、path、resource identity は避ける | 壊れた JSON、JSON でない JavaScript、表示用か判別できない literal |
| XML | 正しい XML 1.0 の character data。entity を decode/escape、tag と属性を保持 | DTD・entity 宣言・XML 1.1。CDATA 本文と属性は翻訳しない |
| INI | 単純な section、key=value の文字列 scalar。コメントとキーを保持 | 複雑な独自 escape・行継続の文法 |
| TOML | 単一行の文字列 scalar。型付き値は保持して翻訳しない | multiline string、複雑な配列・テーブル値を包括的に訳すこと |
| 単純 YAML | mapping の scalar、単純引用文字列、indent による親 mapping | block scalar `|`/`>`、anchor/alias、tag、flow collection、複雑な YAML grammar |
| prose | 本文である確度の高い自然文 | コードらしい内容、構造が壊れた text、曖昧な機械データ |

「無損失」は対応する元構造と非置換領域についての説明です。意味が同じ、モデルが完璧、あらゆる syntax が扱える、という保証ではありません。元の Shift-JIS などで訳文を表現できなければ失敗し、勝手に UTF-8 へ切り替えません。エンコーディング判定自体も推定を含みます。

## ディレクトリのカバレッジ

```bash
attx run --input ./resources --engine auto --src ja --dst zh
```

相対構造を維持して `resources/translated-zh/` に出力します。認識できないファイルは対象ツリー内で原バイトのままコピーします。言語 suffix 付きの source asset も削らずコピーします。symlink は辿らず、cache、`.attx*`、`.git`、node_modules、生成済み translated directory、backup などを除外します。出力パス上の symlink も拒否します。

抽出レポートの `auto_coverage` は `supported_files`、`copied_files`、`unsupported_total`、最大 50 件の `unsupported_paths`、`excluded_entries`、最大 50 件の `excluded_paths` を含みます。excluded は symlink、metadata、backup、除外 directory などで、supported/copy とは別です。除外 directory は中の各 file を数えず 1 entry と数えます。これは形式の対応範囲であり、訳文の成功率ではありません。

ディレクトリの auto は、中の各ファイルを専用アダプターに振り分ける dispatch ではありません。そこにある EPUB、DOCX、字幕、スクリプトを全部専用形式として翻訳するわけではありません。対象を個別に run するか、既知の外部抽出器を使ってください。

## エンコーディングと外部抽出

通常の text 入力は厳密 UTF-8、BOM 付き UTF-16、chardetng の推定と encoding_rs decode を使います。多くの専用 text アダプターと custom profile の出力は UTF-8 です。ZIP/XML document と auto ではそれぞれの経路を使うため、すべての形式に同じ出力 encoding を約束できません。

未認識の text は[プロファイル](profiles.md)、binary・暗号化・非対応 archive は既知の外部抽出器による[JSONL](workspace.md)を使います。PDF、画像 OCR、汎用 executable 翻訳、任意ゲーム binary の書き戻しは組み込み能力ではありません。
