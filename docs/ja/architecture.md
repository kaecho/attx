# アーキテクチャ

attx は binary-only の Rust プロジェクトです。ライブラリーとして公開した async API やバックグラウンドサービスはありません。HTTP は reqwest blocking、並列実行は標準の scoped thread、cache は bundled SQLite です。形式処理とネットワーク処理を分離しています。

## データの流れ

```text
main: 引数・設定・JSON・終了コード
  ↓
pipeline: identity・extract・cache・translate/repair・writeback
  ├─ adapter/profile: input → TextUnit
  ├─ knowledge/preserve/glossary: filter・literal・補助指示
  ├─ llm: batch・context・同期要求・有限 retry
  ├─ quality/review: 正規化・検証・修復対象
  ├─ store: source と translation の進捗
  └─ fileio: lock・backup・stage・置換
```

TextUnit は形式に依存しない本文単位です。id、engine、domain、location、item_type、role、original_lines、source_line_paths、context、payload を持ちます。location は利用者にも見える source anchor、payload は adapter が復元するための追加情報です。

Translation は unit_id、translation_lines、source_hash、passthrough を持ちます。原文 identity と違う訳は再利用できません。WorkspaceMeta は engine、game_path、content_root、語向、created_at を保存し、workspace の内部 metadata は抽出 snapshot/profile digest も保持します。SQLite の published table は実際に書いた行を記録し、repeat custom overwrite の source/live 関係を検査するために使います。JSONL は明示 id/text の外部交換 surface です。

units table の ordinal は抽出順を保存し、不透明な JSONL ID でも原文順を保ちます。数字 locator は scene 内で自然順に扱います。有効行は同じ batch の失敗項目の retry 完了前に caller sink へ渡します。設定 credential を含むモデル content は拒否し、HTTP error の一致する secret は伏せます。

## 全モジュールの役割

| ファイル | 責務 |
|---|---|
| [main.rs](https://github.com/kaecho/attx/blob/main/src/main.rs) | clap CLI と各 subcommand、client 選択、run orchestration、最終 JSON と exit0/1/2 |
| [config.rs](https://github.com/kaecho/attx/blob/main/src/config.rs) | serde TOML schema、既定値、設定探索、例の生成 |
| [pipeline.rs](https://github.com/kaecho/attx/blob/main/src/pipeline.rs) | workspace init、抽出更新、cache 利用、有限 repair、source 検査、profile inference、writeback/report |
| [model.rs](https://github.com/kaecho/attx/blob/main/src/model.rs) | TextUnit/Translation/WorkspaceMeta/JsonlRecord、hash、script heuristic |
| [store.rs](https://github.com/kaecho/attx/blob/main/src/store.rs) | WAL SQLite の meta/units/translations/published、原文一致取得、公開済み行の追跡、transaction 保存と件数 |
| [llm.rs](https://github.com/kaecho/attx/blob/main/src/llm.rs) | translation prompt profile、batch と scene context、rate clock、scoped worker、HTTP/SSE、response decode、失敗項目 retry、JSON helper |
| [quality.rs](https://github.com/kaecho/attx/blob/main/src/quality.rs) | visible text、script 残留、kana 分類、Chinese 正規化、行/control 検証 |
| [review.rs](https://github.com/kaecho/attx/blob/main/src/review.rs) | cache の機械レビュー、bucket と sample、repair target の全件選択 |
| [preserve.rs](https://github.com/kaecho/attx/blob/main/src/preserve.rs) | builtin/workspace regex、左端最長 span の mask と literal 復元 |
| [glossary.rs](https://github.com/kaecho/attx/blob/main/src/glossary.rs) | 候補抽出・原文検証・上限・投票、glossary TOML、batch 注入と advisory check |
| [knowledge.rs](https://github.com/kaecho/attx/blob/main/src/knowledge.rs) | 経験 schema と未知 kind 保持、embedded/global/workspace layer、抽出済み unit の filter、prompt note |
| [learn.rs](https://github.com/kaecho/attx/blob/main/src/learn.rs) | 証拠集計、pending 提案、承認/却下、note 管理と report |
| [profile.rs](https://github.com/kaecho/attx/blob/main/src/profile.rs) | TOML ルール、CustomAdapter、保存/検出、行 span と JSON anchor |
| [fileio.rs](https://github.com/kaecho/attx/blob/main/src/fileio.rs) | OS workspace lock、private staging/backup、cap-std の開いた directory handle に結び付けた作成と rename、初回 backup 検査、per-file replacement |
| [textio.rs](https://github.com/kaecho/attx/blob/main/src/textio.rs) | UTF-8/BOM/推定 encoding の text decode |
| [defaults/rmmz.toml](https://github.com/kaecho/attx/blob/main/src/defaults/rmmz.toml) | binary に埋め込む RMMZ 抽出経験 |

## アダプターの module map

| ファイル | 形式 |
|---|---|
| `adapter/mod.rs` | FormatAdapter trait、登録順、強制選択、OutputFile、共有 JSON path・出力名 helper |
| `adapter/auto.rs` | byte span JSON/XML/scalar/prose、encoding roundtrip、directory copy と coverage |
| `adapter/rmmz.rs` | ゲーム root、DB/event、source backup、固定 slot と speaker layout |
| `adapter/rmmz_plugins.rs` | plugins.js の parameter と nested JSON、機械 literal の除外 |
| `adapter/epub.rs` | EPUB ZIP と HTML/XHTML 本文、ruby、言語 metadata |
| `adapter/docx.rs` | WordprocessingML の段落・脚注・文末脚注 |
| `adapter/xlsx.rs` | spreadsheet shared string と発音要素 |
| `adapter/subtitle.rs` | SRT、VTT、LRC の timestamp と本文 |
| `adapter/ass.rs` | ASS/SSA Dialogue field と override/control |
| `adapter/csv.rs` | CSV/TSV の quoted cell parsing と置換 |
| `adapter/po.rs` | gettext 単数 entry の msgstr |
| `adapter/renpy.rs` | Ren'Py translate block と old/new |
| `adapter/plaintext.rs` | TXT/Markdown の行、fenced code と prefix の保護 |
| `adapter/jsonkv.rs` | MTool、Paratranz、VNTextPatch、i18next の形 |
| `adapter/jsonl.rs` | 外部 id/text、file/directory の出力 |
| `adapter/xmllite.rs` | XML text/escape の共有 helper |

ソース一覧は [src/adapter](https://github.com/kaecho/attx/tree/main/src/adapter) にあります。xmllite は helper で、単独の公開 `xml` adapter ID ではありません。

## 拡張境界

`FormatAdapter: Send + Sync` は id、label、extensions、input_kind、detect、extract、writeback を定義します。extract は入力を TextUnit にし、writeback は translation map を適用した `Vec<OutputFile>` を返します。adapter が勝手に API を呼んだり、render 中に target file を置換したりしません。pipeline が source validation と disk write を管理します。

形式を追加する軽い手段は custom profile、複雑な grammar/encoding/anchor には専用 Rust adapter です。どちらも TextUnit と output contract を共用します。auto は unsupported file を copy できますが、専用 adapter の directory dispatch ではありません。

## 失敗時の境界

恒久的 HTTP configuration/auth error は fatal class にし、待機要求を止めます。成功済み batch の cache は維持します。model 応答失敗は有効 ID を保持し、失敗 unit だけ有限 retry/repair します。review の sample 上限は表示だけで、候補検査を 40 件に切り詰めません。

writeback は normalize/reflow を in-memory で計画し、source hash、anchor、全抽出集合の snapshot を確認します。profile 内容は workspace の digest で固定し、run は全段階を一つの workspace lock で処理します。default block と dry-run は cache/output を変更しません。正常 writeback 後は変更訳を cache に同期します。artifact を全件 stage しても、複数 target の置換は directory transaction にはなりません。

出力と完成した backup の permissions は原文から引き継ぎ、temporary は Unix では0600で作ります。cap-std 4 は staging、backup、rename を開いた directory handle に結び付け、same-file 1 で親directoryの同一性も確認します。OutputFile は path/bytes に加えて任意 permissions を持ちます。published は過去に公開した訳を区別し、output_paths は実際に生成した副本だけを次のcustom directory scanから除外します。

## 設計上の参考元

[LinguaGacha](https://github.com/neavo/LinguaGacha) の v0.125.0、commit [5a0058d57abfc6df371339818894097f3879bd28](https://github.com/neavo/LinguaGacha/tree/5a0058d57abfc6df371339818894097f3879bd28) では、失敗 unit ごとの retry limit、duplicate/omission rejection、line/control 検証、script 評価、shared batch/agent API、source と work の分離、temporary replacement を確認しています。

attx はそれらの設計上の考え方を参考に、同期 Rust の pipeline と CLI 契約として独立に実装しています。上流コードをコピーしたり、Electron runtime、PDF 処理、key rotation、未知形式一般推論を上流から取り込んだと主張しません。調査した上流 README は商用利用の連絡・承認を求めており、root LICENSE は確認できていません。OSI license や互換性を断定しません。attx 自身の MIT LICENSE と上流の条件は別です。

拡張実装の手順は[開発](development.md)、運用上の invariant は[ワークスペース](workspace.md)を参照してください。
