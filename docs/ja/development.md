# 二次開発と保守

attx は Rust 2024、MSRV 1.89 の binary-only プロジェクトです。ネットワーク不要の単体テストと local fixture を基準に開発できます。通常の test suite に実 API key や有料 model access を要求しないでください。[アーキテクチャ](architecture.md)に全 module map があります。

## 開発環境

```bash
git clone https://github.com/kaecho/attx.git
cd attx
cargo build --locked
cargo test --locked -- --test-threads=1
cargo run -- --help
cargo build --release --locked
```

これらは開発者が実行するチェック例です。1.89 で使えない language/library feature を無断で入れず、rust-version を変更する場合は release と文書を合わせて更新します。release profile は LTO、codegen-units=1、strip を使います。Windows/Linux の I/O、PATH、改行、backup replacement を意識してください。

test-threads=1 は環境変数を変更する fixture の競合を避けるためにも有用です。parallel HTTP の仕様を確認する場合も実 provider ではなく loopback の deterministic mock server を使います。doctor の `--ping` は手動 network check で、通常 CI smoke とは分けます。

## 最初に拡張手段を選ぶ

- 一行 span や既知 JSON path で十分なら[プロファイル](profiles.md)を作る。
- 既に抽出された field の skip/keep 判断なら[経験 layer](usage.md)を使う。
- control literal の追加なら preserve rule、固有名詞なら glossary、文体なら prompt note。
- 独自 parser、escape、archive、複数の anchor、encoding が必要なら専用 adapter。
- binary の仕様がなければ、既知外部 extractor と JSONL の contract を用意する。

一つの問題に第二の pipeline や独自 agent retry loop を作らず、既存の共通処理へ統合します。

## 専用アダプターを追加

1. `src/adapter/<name>.rs` を追加し、既存の近い形式を参考にします。
2. `FormatAdapter` の `id` と `label`、必要なら `extensions`、`input_kind`、`detect` を実装します。ID は workspace に保存する安定識別子です。
3. `extract` で表示用 source のみを TextUnit にします。location は安定で一意、source_line_paths は復元する行 anchor、context は scene 境界、role は話者です。
4. `writeback` は入力と有効 translation map から `Vec<OutputFile>` を返します。各 output は path/bytes と任意 permissions を持ち、source の権限を引き継ぎます。訳のない unit は source を保持し、control と構造を維持します。render 中に直接 target を書いたり backup を省略しません。
5. `adapter/mod.rs` に module と `all_adapters` 登録を追加します。検出順の競合を考慮し、汎用 sniff が特定形式を横取りしないようにします。
6. 必要な場合のみ llm の format prompt profile、preserve の markup/control、writeback の layout 正規化、coverage report と接続します。
7. extract、identity、no-op/source roundtrip、marker translation、partial/source fallback、encoding、broken input、書き戻し拒否を test します。
8. formats/CLI/人間文書/Skill の全 locale と CHANGELOG を更新します。

trait の全契約は [adapter/mod.rs](https://github.com/kaecho/attx/blob/main/src/adapter/mod.rs) です。アダプター自身はネットワークを呼びません。source 変更・全抽出集合の変化を pipeline が検査できる payload/anchor を設計します。

SQLite の published table は実際に書いた行を追跡します。repeat overwrite と JSONL repair の経路では source/cache/live/published の関係を保ち、無関係な live key/comment を巻き戻さないようにします。fileio は private staging/backup と cap-std 4 の open directory handle を使います。adapter に独自 pathname ベースの書き込みを追加してこの保護を回避しません。

元構造を byte 単位で保つ必要があるなら auto の span と codec を参考にしますが、曖昧な grammar を prose として受け入れる fallback を追加しません。対象 encoding が訳文を表せない場合は明示 failure にします。

## プロファイルを追加

```bash
attx profile new --output ./fmt.toml --name format-name
attx profile test --profile ./fmt.toml --input ./fixture --src ja --roundtrip
attx profile save --profile ./fmt.toml
```

name、extension、detect_regex、min_units、rule kind、overwrite と escape の範囲を確認します。template と examples は `profiles/examples/` にあります。既存 saved profile を --force で変える操作は必要な対象だけに限定します。workspace の profile snapshot/digest は既存 anchor の identity なので、推論後に overwrite を手で切り替えて使うことはできません。

model infer は宣言型の rule のみ、最大 3 提案/検証、overwrite=false、原文 decoded no-op roundtrip です。これをモデル生成 parser の任意実行へ広げないでください。

## 設定キーを追加

`config.rs` の適切な section に typed field と serde default を定義し、Default 実装も揃えます。`example_toml()` と配布 `setting.example.toml` の値・説明を同時更新します。field を置くだけではなく利用箇所で意味を実装し、0、上限、省略、client.extra の優先順を明示します。

request を変える値は llm に、翻訳 flow の値は pipeline に、format の値は profile/adapter に置きます。設定探索 path と秘密情報の扱いを勝手に追加しません。request/report に key を漏らさず、config schema test と変更挙動の offline test、全 locale の[設定](configuration.md)を更新します。

## CLI と report を追加

main の clap declaration、dispatch、pipeline 実装、report type と Serialize、終了コードの判定を一組で変更します。グローバル flag は既存 config/client の pattern に合わせます。未完了の structured result を exit0 にして隠したり、正常 partial を fatal exception にして JSON を捨てたりしません。

新 field は意味、count scope、dry-run、optional serialization を文書化します。command-specific help、[CLI](cli.md)、Skill の cli-command-contract、関連する error recovery と READMEs の記述を同期します。モデル schema や JSONL を変える場合は既存 import/export caller も移行します。

## テストの観点

| 領域 | offline fixture で検査する内容 |
|---|---|
| adapter | detect 順、抽出 domain、機械 literal の除外、anchor、構造保持、unsupported 入力 |
| encoding | UTF-8/UTF-16 BOM/legacy、CRLF、末尾 newline、unrepresentable target |
| model/quality | ID、line count、duplicate/omission、control の個数と順、Chinese edge/mixed/untranslated、保護 glyph |
| cache | source hash、既存成功の再利用、追加/消失 source、workspace 語向/engine/profile digest |
| HTTP | local mock で有限 retry、failed-only split、429、fatal 401、truncation、SSE、成功 cache 維持 |
| writeback | dry-run 非変更、blocked、backup failure、stage、per-file atomic、partial 原文 fallback |
| RMMZ | command count、speaker 第 1 slot、control adjacency、最後の slot overflow、再抽出原文 |
| JSONL | 先行全件 validation、unknown/duplicate alias/source mismatch、transaction、空監査訳 skip |
| knowledge | layer 優先、pending skip inert、suffix/exact、unknown kind 保持、prompt note scope |

機械 test だけで言語品質やゲーム画面の正常を保証できないため、ユーザーの代表的入力での手動確認は別の evidence として報告します。有料 provider を呼んだ検証は明示し、キーや private source を fixture にコミットしません。

## CI と release

[ci.yml](https://github.com/kaecho/attx/blob/main/.github/workflows/ci.yml) は main/master push と pull request に対し、Ubuntu/Windows の stable Rust で release build、cargo test、CLI doctor/help smoke を行います。doctor smoke は ping を付けません。

[release.yml](https://github.com/kaecho/attx/blob/main/.github/workflows/release.yml) は `v*` tag push または workflow_dispatch で起動します。verify job が locked tests、release binary、tag と version の一致、help/doctor、strict docs build を確認してから 4 target matrix を build します。Linux x86_64、Windows MSVC x86_64、macOS aarch64/x86_64 の binary を作り、Windows は zip、その他 tar.gz にまとめます。

package には README 2 言語、CHANGELOG、LICENSE、setting.example.toml、skills、profiles、docs、mkdocs.yml、requirements-docs.txt を含めます。publish はすべての package を集めて GitHub Release を作成し、CHANGELOG を説明に使います。

release 手順は package version と Cargo.lock、CHANGELOG、設定テンプレート、全 locale、Skill を同期し、チェック後に commit と `v<version>` tag を公開する流れです。tag と package version を一致させ、公開済み release を検証せず書き換えないでください。

## 文書サイトと翻訳

MkDocs の source は `docs/en/`、`docs/zh/`、`docs/ja/` です。全 locale は同じ 15 filename を持ちます。index、install、quickstart、configuration、usage、formats、profiles、rmmz、quality、cli、agents、workspace、architecture、development、troubleshooting を揃えます。

```bash
python -m venv .venv-docs
. .venv-docs/bin/activate
pip install -r requirements-docs.txt
mkdocs serve
mkdocs build --strict
```

PowerShell の activation は `.\.venv-docs\Scripts\Activate.ps1` です。site output を repository の既存 site/ と分けたい場合は `mkdocs build --strict --site-dir <temporary-directory>` を使います。

`mkdocs.yml` の nav と i18n 設定を変更ページに合わせます。locale 内リンクは相対 filename、source/Skill は安定 GitHub URL を使い、個人の絶対パスや agent artifact URI は書きません。command、flag、JSON field は翻訳せず、説明は各言語で自然に書きます。既定値、version、permission、cost、limit、error path を全 locale に同じ密度で反映します。

[docs.yml](https://github.com/kaecho/attx/blob/main/.github/workflows/docs.yml) は docs/nav/README などの変更を対象に strict build し、Pages artifact を公開します。`site/` の生成物を手で直して source の不整合を隠しません。人間文書の変更が agent protocol に影響する場合は `skills/attx/` も同時に更新します。

## 上流の考え方を扱う

LinguaGacha の失敗項目 retry、line/script check、agent/batch 共有、source/work 分離は[アーキテクチャ](architecture.md)に source evidence と参考範囲を記載しています。attx の実装は独立 Rust です。上流 source をコピーしたり、上流の商用条件・license を attx の MIT と同じと見なしたりしません。
