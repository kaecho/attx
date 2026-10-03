# CLI リファレンス

```text
attx [--config <path>] [--client <name>] <command> [options]
```

基準は実行中の `attx --help`、各 subcommand の `--help`、最終 stdout JSON です。[エージェント契約](https://github.com/kaecho/attx/blob/main/skills/attx/references/cli-command-contract.md)も同じ処理を使います。stderr の batch progress を結果として parse しないでください。

## グローバルオプション、既定値、終了コード

| オプション | 効果 |
|---|---|
| `--config <path>` | 設定パス。指定しなければ存在する $ATTX_HOME/setting.toml、次に ./setting.toml |
| `--client <name>` | この実行で llm.default_client の代わりに client を選択 |
| `-h`, `--help` | CLI または subcommand の help |
| `-V`, `--version` | binary version |

input オプションを持つ detect、analyze、init、run、profile infer/test は `--game` を input の別名として受け付けます。原文言語の既定値は `ja`、対象言語は `zh`。label は workspace で小文字化し `_` を `-` に正規化します。ASCII 英数字、`-`、`_` の語タグを使います。ヘルプの ja/en は例であり、言語判定は script heuristic です。

| 終了コード | 読み方 |
|---|---|
| `0` | 操作完了、または dry-run 計画。review に問題がないという保証ではない |
| `1` | 致命的な設定、HTTP、I/O、source identity、import validation などのエラー。stderr に error、完全な stdout JSON があるとは限らない |
| `2` | 非 dry-run の translate/repair/translate-jsonl 未完了、writeback/run の blocked/partial。結果 JSON を保持して読む |

clap の引数解析エラーも 2 を使うため、JSON 結果がある不完全状態と区別します。doctor の ping 失敗、profile test の `roundtrip.ok=false`、review の警告は report の内容を見る必要があります。`status: "ok"` だけで API 接続・意味の完璧さを判断しません。

## 調査コマンド

| 構文 | 処理と report |
|---|---|
| `doctor [--ping] [--json]` | 通常は人間向け出力。json は llm、ping、adapters、saved_profiles、status。llm は configured、選択 name/model/base_url または error。キーは出さない。ping は小さな API 要求で、結果文字列を確認する |
| `formats` | formats[]。各項目は id、label、extensions、input。saved custom は profile path も持つ |
| `detect --input <path>` | engine、content_root、label、profile。専用、saved profile、auto の順。detect に --engine はない |
| `analyze --input <path> [--src ja]` | builtin_detect、saved_profile_detect、details、next_steps。file は size、binary/container、encoding、行数、JSON shape、sample。directory は files、extensions、sample_files、peek |

`doctor` は存在しなければカレントディレクトリに setting.example.toml を作ります。detect/analyze はモデルを呼ばず、sample 内の指示を実行しません。

## `profile`

| サブコマンド | 全オプションと意味 |
|---|---|
| `new --output <file> [--name myformat]` | コメント付き template。既存 output を拒否 |
| `infer --input <path> --output <file> [--src ja] [--name auto-profile]` | 有料推論。最大 3 提案/検証。overwrite=false、既存 output を拒否 |
| `test --profile <path-or-name> --input <path> [--src ja] [--limit 10] [--roundtrip]` | 全件 trial extract と上限付き sample。limit は sample 件数。roundtrip は marker translation によるメモリ検査のみ |
| `save --profile <file> [--force]` | user profile directory へ保存。同名を変える場合 force が必要 |
| `list` | 保存済み profile と directory を一覧 |

report:

- new: `written`、`next`、`status`。
- infer: `profile`、`engine`、`units`、`attempts`、`output`、`roundtrip`、`overwrite`、`status`。
- test: `profile`、`engine`、`units`、`sample[{location,role,text}]`、`detects`。roundtrip が実行されれば `{ok,output_files,outputs}`、失敗は `{ok:false,error}`。
- save: `saved`、`status`。
- list: `profiles[{name,engine,label,extensions,path}]`、`dirs`。

推論検証は全 semantics の証明ではありません。[プロファイル](profiles.md)を参照してください。

## workspace パイプライン

### `init`

```text
init --input <path> [--engine <id>] [--profile <path-or-name>]
     [--src ja] [--dst zh] [--workspace <dir>]
```

workspace を作成または同一性を確認して再利用します。report は `workspace`、`status`。engine は形式一覧の ID、保存 profile は `custom:<name>` です。profile を使うと workspace に snapshot をコピーし、digest で内容を固定します。コピー後に overwrite を手で切り替えることも拒否します。入力・engine・語向・profile を別条件へ変更して既存 DB を借用することはできません。

### `extract`

```text
extract --workspace <dir> [--no-knowledge]
```

原文を更新し、source 一致の translation cache を維持します。no-knowledge は experience filter を使わず、形式パーサーまで無効にするわけではありません。report は `extracted`、`skipped_by_knowledge`、`rules_applied`、`status`。auto は `auto_coverage{supported_files,copied_files,unsupported_total,unsupported_paths,excluded_entries,excluded_paths}` を追加し、各 paths sample は最大 50 件です。除外 directory は 1 entry と数え、symlink/metadata/backup の除外は copied/supported と分けます。

### `translate` と `repair`

```text
translate --workspace <dir> [--limit N] [--dry-run] [--retry-passthrough]
repair --workspace <dir> [--limit N] [--dry-run]
```

translate は pending と機械レビュー対象を処理し、retry-passthrough は原文仮置きを再 queue します。repair は pending、passthrough、残留、control、namebox 問題を有限に修復します。limit はユニット上限、dry-run は HTTP と translation cache 更新なしの計画です。

共通 TranslateReport:

| フィールド | 意味 |
|---|---|
| `pending_before` | 実行前の有効訳なし件数 |
| `translated` | 本操作で pending から解決した成功数。passthrough は含めない |
| `pending_after` | 最終 pending |
| `passthrough` | 最終原文仮置き数 |
| `dry_run` | 計画のみか |
| `skipped_note` | 停止・未解決などの説明 |
| `status` | ok または needs_attention |
| `planned` | 選ばれた初期対象数 |
| `repaired` | 初期レビュー問題から解決した数 |
| `repair_rounds` | 実行した修復パス数 |
| `unresolved` | 最後に残る修復対象数 |
| `normalized_lines` | Chinese 正規化で変わった行の signal |
| `review` | 最終機械レビュー |

pending と review の問題は重複し得るため、全値を足して単純な総件数にしません。モデル retry と repair の上限は[設定](configuration.md)にあります。

### `writeback`

```text
writeback --workspace <dir> [--dry-run] [--no-learn] [--allow-partial]
```

source/hash と anchor を検査し、正規化・固定 slot reflow・完全性検査の後に書きます。通常翻訳依頼の書き戻しに追加 permission はありません。未解決があれば既定で block します。allow-partial はユーザーが未解決原文を残す output を認めた場合のみ使用します。no-learn は書き戻し後の経験まとめを省略します。

| フィールド | 意味 |
|---|---|
| `files` | 書いたファイル数。dry-run は計画 |
| `units_applied` | 有効訳を適用した数 |
| `paths` | 出力 path 配列。dry-run は予定 |
| `dry_run` | true なら output/cache を変更しない |
| `status` | ok、blocked、needs_attention |
| `skipped_note` | 未適用理由 |
| `units_skipped` | pending/invalid などで未適用の数 |
| `normalized_lines` | Chinese 正規化 signal |
| `reflowed_units` | RMMZ fixed slot の再配置数 |
| `overflow_lines` | 表示幅からはみ出す行の signal |
| `review` | 最終レビュー |
| `learned` | 自動経験まとめ実行時のみ SummaryReport |

backup 失敗時は置換しません。per-file atomic であり、directory transaction ではありません。blocked と dry-run で cache の正規化を永続化しません。

### `run`

```text
run --input <path> [--engine <id>] [--profile <path-or-name>]
    [--src ja] [--dst zh] [--workspace <dir>] [--limit N]
    [--no-translate] [--no-writeback] [--glossary | --no-glossary]
    [--no-infer] [--allow-partial]
```

init/extract/任意 glossary/translate/有限 repair/writeback を統合します。`no-translate` は抽出と確認のみで通常 writeback も行いません。`no-writeback` は cache まで。`no-infer` は有料 profile 推論だけを止め、auto は有効です。`glossary` は設定が無効でも有料構築、`no-glossary` は単発無効で、両者は競合します。`limit` は全量完了の意味ではなく、sample は通常 `no-writeback` と使います。run に `dry-run`、`retry-passthrough`、`no-learn` はありません。

report は `workspace`、`extracted`、`extract`、`status` と、実行された `profile_inference`、`glossary`、`translate`、`review`、`writeback` です。glossary 通常失敗は `error` を記録し、致命的認証エラーは止まります。最終状態は実際の writeback/translation の blocked または needs_attention を反映します。

### `status` と `review`

```text
status --workspace <dir>
review --workspace <dir>
```

status は `engine`、`game_path`、`source_lang`、`target_lang`、`total`、`translated`、`pending`、`passthrough`、`domains{domain:{total,translated}}`。

review は `total`、`translated`、`pending`、`passthrough`、`glossary` と `residual_source`、`kana_edge`、`kana_mixed`、`kana_untranslated`、`identical`、`control_loss`、`namebox_mismatch`。各 bucket は `{count,sample:[{location,unit_id,detail}]}`、最大 40 sample。kana 分類は排他的、他 bucket と集約 residual は重複し得ます。review はモデルを呼ばず、警告があるだけで通常の操作終了コードを変えるものではありません。

## JSONL コマンド

```text
translate-jsonl --input <file> --output <file> [--src ja] [--dst zh] [--limit N]
export-jsonl --workspace <dir> --output <file> [--filter pending]
import-jsonl --workspace <dir> --input <file>
```

translate-jsonl は workspace なしで id/text と optional context/role/item_type を翻訳し、translation と translation_lines を付加します。report は TranslateReport、Translator の retry のみで workspace repair/resumption はありません。不完全なら 2。出力置換は backup/stage を使います。

export filter は pending、all、translated、passthrough。report は `exported`、`output`。import は `imported`、`status`。全 import を先に検証して transaction 保存し、未知 ID、source mismatch、同じ unit の duplicate alias、構造/control 不一致を拒否します。空の監査訳は skip します。詳しい schema は[ワークスペース](workspace.md)。

## `preserve`

```text
preserve list --workspace <dir>
preserve add --workspace <dir> --pattern <regex> [--info ""]
preserve remove --workspace <dir> --pattern <regex>
```

list は `engine`、`count`、`rules[{pattern,info,source}]`、add は `added`、`file`、`status`、remove は `removed`、`pattern`、`status`。remove は pattern の完全一致で workspace rule を削除します。invalid regex と空に一致する regex は拒否します。内蔵保護は常時利用します。

## `glossary`

全 subcommand は `--workspace <dir>` を必要とします。

| サブコマンド | 追加オプション |
|---|---|
| `build` | `[--min-occurrences N] [--dry-run]` |
| `list` | `[--all]` |
| `add` | `--src <term> --dst <name> [--info ""] [--case-sensitive]` |
| `remove` | `--src <term>` |
| `import` | `--file <json>` |
| `export` | `--file <json>` |
| `check` | なし |

build は有料で enabled=false でも明示実行できます。dry-run は要求計画のみです。report は `candidates`、`above_threshold`、`truncated`、`asked`、`added`、`rejected`、`total_active`、`min_occurrences`、`dry_run`、`file`、任意の `sample`。asked は extract batch 数で dry-run では予定、truncated は候補上限による切り詰めの signal です。最大 40 batch という原文 coverage の限界も別にあります。

list は `source_lang`、`target_lang`、`count`、`terms`。all は rejected も表示。各 term は src/dst/info/count/status/source/case_sensitive。add は `added`、`file`、`status`、remove は `removed`、`src`、`status`。import は `imported`、`status`、export は `exported`、`file`、`status`。

check は `active_terms`、`terms_seen`、`terms_fully_applied`、`violations[{src,dst,occurrences,applied}]`。substring の advisory で語形変化による誤報があり、hard rejection ではありません。glossary がない check は構築/import を求めるエラーです。

## `learn`

| サブコマンド | 全オプション |
|---|---|
| `summarize` (別名 `scan`) | `--workspace <dir> [--llm]` |
| `note` | `--text <instruction> [--topic prompt] [--name ""] [--workspace <dir>] [--format <id>]` |
| `pending` | なし |
| `review` | `[--approve 1,3] [--reject 2] [--approve-all]` |
| `list` | `[--format <id>] [--workspace <dir>]` |
| `defaults` | `--format <id>` |
| `forget` | `--field <field> [--format <id>]` または `--name <name> [--format <id>] [--workspace <dir>]` |

summarize の普通実行は無料の証拠集計、llm は有料審査です。SummaryReport は `format`、`units_scanned`、`fields_seen`、`entries_written`、`pending`、`notes`、`file`。note report は `format`、`topic`、`name`、`source`、`text`、`file`、`layer`、`reaches_prompt`。prompt だけが翻訳に入ります。

pending は `pending` と `entries`。entry は `index`、`format` と証拠・field 情報を持ちます。review は `approved`、`rejected`、`remaining`、`files_written`。1-based index の comma list を使い、approve/reject がなければエラーです。approve-all は存在しますが、エージェントの独断使用は禁止です。

list は `formats`、`knowledge`、`note`。knowledge は format/version/kinds/entries、workspace 指定なら layer も含みます。embedded default は含まれず defaults が TOML として出力します。forget は `forgotten` と `field` または `name`、`status`。field と name はどちらか一方、workspace は note にだけ適用できます。

scope、schema、破壊的 skip 承認は[使い方](usage.md)、自動実行時の停止条件は[エージェント](agents.md)を参照してください。
