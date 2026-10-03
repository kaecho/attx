# インストール

## リリースバイナリ

[GitHub Releases](https://github.com/kaecho/attx/releases) の `v0.10.0` から OS と CPU に対応するアーカイブを選びます。配布ターゲットは Linux x86_64、Windows x86_64、macOS aarch64 と x86_64 です。Windows は ZIP、その他は tar.gz です。

配布物には `attx` または `attx.exe`、英語と中国語の README、`CHANGELOG.md`、attx の `LICENSE`、`setting.example.toml`、`skills/`、`profiles/`、`docs/`、`mkdocs.yml`、`requirements-docs.txt` が含まれます。実行ファイルを PATH に入れるか、絶対パスで呼び出します。

POSIX:

```bash
chmod +x ./attx
./attx --version
./attx --help
```

PowerShell:

```powershell
.\attx.exe --version
.\attx.exe --help
```

PowerShell はカレントディレクトリの実行ファイルを名前だけでは検索しません。PATH に追加していなければ、以後の例の `attx` を `.\attx.exe` に置き換えます。実行時に Python、Node.js、Rust、MCP サーバーを起動する必要はありません。

## ソースからビルド

Rust の最低対応バージョンは 1.89 です。edition は 2024、nightly は不要です。

```bash
git clone https://github.com/kaecho/attx.git
cd attx
cargo build --release
./target/release/attx --help
cargo install --path .
```

Windows のビルド結果は `target\release\attx.exe` です。MSVC ターゲットでは Rust ツールチェーンと対応する C/C++ ビルド環境を用意してください。`cargo install --path .` を使う場合は Cargo の bin ディレクトリを PATH に入れます。ビルドと開発環境の詳細は[開発](development.md)にあります。

## LLM の設定

POSIX:

```bash
cp setting.example.toml setting.toml
chmod 600 setting.toml
```

PowerShell:

```powershell
Copy-Item .\setting.example.toml .\setting.toml
notepad .\setting.toml
```

`setting.toml` の `base_url`、`api_key`、`model` を本機で編集します。キーをチャットに貼り付ける必要はありません。Windows ではファイルのアクセス権を確認し、他ユーザーから読めない場所に置いてください。

```bash
attx doctor --json
attx doctor --json --ping
```

`--ping` は小さな有料 API 要求を送る場合があります。`llm.configured` と `ping` の内容を確認してください。`doctor` の終了コードやトップレベルの `status: "ok"` だけでは接続成功を判定できません。`doctor` はカレントディレクトリにない場合 `setting.example.toml` を生成することもあります。

検出や抽出などネットワーク不要の操作は、設定ファイルなしでも実行できます。設定が存在する場合、無効な TOML は読み込みエラーになります。設定の全キーと検索順序は[設定](configuration.md)を参照してください。

## データの保存先

- 設定: `--config`、存在する `$ATTX_HOME/setting.toml`、`./setting.toml` の順。
- 保存済みプロファイル: `$ATTX_HOME/profiles/` と OS のユーザー設定ディレクトリにある `attx/profiles/`。
- 形式ごとの経験: 同じ基準の `knowledge/`。
- ワークスペース: 通常は入力内または入力の隣。`--workspace` で別の書き込み可能な場所を指定できます。

Linux のユーザー設定ディレクトリは通常 `~/.config`、macOS は `~/Library/Application Support`、Windows は `%APPDATA%` です。設定ファイル自身の検索に、これらの OS 設定ディレクトリが自動で使われるわけではありません。

## 次に進む

[クイックスタート](quickstart.md)で実際の入力を翻訳できます。エージェント経由なら[エージェント](agents.md)、特定形式の制約は[フォーマット](formats.md)を先に確認してください。
