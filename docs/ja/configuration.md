# 設定リファレンス

`setting.toml` は TOML です。[配布テンプレート](https://github.com/kaecho/attx/blob/main/setting.example.toml)と [実装](https://github.com/kaecho/attx/blob/main/src/config.rs)が基準です。既存の正常な設定をテンプレートで上書きする必要はありません。

## 設定の検索とスコープ

1. `--config <path>` があればそのパスを使用します。
2. 指定がなければ、存在する `$ATTX_HOME/setting.toml` を使用します。
3. それ以外は実行時の `./setting.toml` を使用します。

指定先も含めてファイルが存在しない場合、非 LLM コマンドを使えるように空の client 一覧と各セクションの既定値で開始します。翻訳などには client の設定が必要です。既存ファイルの構文エラーは無視しません。

設定ファイルが存在する場合、`[llm]`、`default_client`、`clients` が必要です。`[translation]`、`[glossary]`、`[learn]` はセクション全体を省略でき、内部の各キーも省略できます。`--client <name>` はその呼び出しだけの選択であり、自動キー切り替えや failover を意味しません。

`ATTX_HOME` は設定・プロファイル・知識の格納ルートです。プロファイルと知識の探索には OS ユーザー設定ディレクトリの `attx/` も使います。新規保存は優先ディレクトリに行います。OS 設定ディレクトリから `setting.toml` を自動検索することはありません。ワークスペースの格納先は別で、`--workspace` または入力から決まります。

POSIX:

```bash
export ATTX_HOME="$HOME/.config/attx"
attx --config ./setting.toml --client main doctor --json
```

PowerShell:

```powershell
$env:ATTX_HOME = "$env:APPDATA\attx"
.\attx.exe --config .\setting.toml --client main doctor --json
```

## 完全な基本例

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"
base_url = "https://api.example.com/v1"
api_key = "YOUR_API_KEY"
model = "provider-model-name"
timeout = 600
# temperature = 0.3
# reasoning_effort = "medium"
# max_tokens = 8192
stream = false
# extra = { top_p = 0.9 }

[translation]
worker_count = 8
rpm = 60
retry_count = 3
retry_delay = 2
batch_chars = 2500
max_context_items = 6
repair_rounds = 2
context_chars = 1200

[glossary]
enabled = false
min_occurrences = 10
max_terms = 200
inject_limit = 30

[learn]
auto_summarize = true
llm_review = false
```

URL、キー、モデルは例示用です。実在する値を本機で設定してください。

## `[llm]` と `[[llm.clients]]`

| キー | 型 | 既定値 | 効果と注意 |
|---|---|---|---|
| `llm.default_client` | 文字列 | 必須 | 使用する client の `name`。テンプレートは `main` |
| `llm.clients` | テーブル配列 | 必須 | `[[llm.clients]]` を複数定義できる。自動巡回はしない |
| `name` | 文字列 | 必須 | `--client` の参照名。一意にする |
| `provider_type` | 文字列 | `"openai"` | 記録用。プロバイダー固有の API へ切り替える値ではない |
| `base_url` | 文字列 | 必須 | 末尾の `/` を除いて `/chat/completions` を付加 |
| `api_key` | 文字列 | 必須 | HTTP Bearer 認証のキー。空や例示用の値は実運用に使わない |
| `model` | 文字列 | 必須 | プロバイダーが公開する正確なモデル名 |
| `timeout` | 非負整数 (`u64`) | `600` | 要求ごとの秒数。実効値は最低 30 秒 |
| `temperature` | 数値、省略可 | 翻訳 `0.3`、JSON helper `0.0` | 明示すればどちらもその値。対応範囲は API 側の仕様 |
| `reasoning_effort` | 文字列、省略可 | 送信しない | `low`、`medium`、`high` など。値の妥当性はプロバイダー依存 |
| `max_tokens` | 非負整数 (`u32`)、省略可 | 送信しない | 出力トークン上限。新しい API では `extra.max_completion_tokens` が必要な場合がある |
| `stream` | 真偽値 | `false` | true なら SSE の `delta.content` を連結して応答を読む |
| `extra` | TOML テーブル | `{}` | JSON 要求に最後に追加・上書き。`messages` だけは無視 |

全 client は OpenAI 互換 Chat Completions 方式です。ネイティブの別プロバイダー API を `provider_type` だけで利用できません。応答も互換の text content または対応 SSE を返す必要があります。`base_url` に `/chat/completions` を含めると二重に追加されるため、API の基底パスを指定します。

## 追加 JSON オプション

```toml
[[llm.clients]]
name = "reasoning"
base_url = "https://api.example.com/v1"
api_key = "YOUR_API_KEY"
model = "provider-reasoning-model"
reasoning_effort = "medium"
extra = { max_completion_tokens = 16384, top_p = 0.9 }
```

または client の直後に `[llm.clients.extra]` を置きます。

```toml
[llm.clients.extra]
max_completion_tokens = 16384
[llm.clients.extra.chat_template_kwargs]
enable_thinking = false
```

ネストしたテーブルは JSON オブジェクトになります。`extra` は `model`、`temperature`、`stream` など既存キーも上書きします。`messages` を指定してもプロンプトは置き換えられません。`extra` に不必要な秘密情報を置かず、未知のフィールドによる HTTP 400 は自動修復されないと考えてください。上限が小さすぎると応答切断を起こし、再試行や修復の費用が増えます。

## `[translation]`

| キー | 型 | 既定値 | 効果 |
|---|---|---|---|
| `worker_count` | 非負整数 (`usize`) | `8` | 同期 HTTP ワーカー数。実効値は最低 1、バッチ数以下 |
| `rpm` | 非負整数 (`u32`) | `60` | 要求開始の共有間隔。`0` は無制限。初回・再試行・分割要求で同じ時計を使う |
| `retry_count` | 非負整数 (`u32`) | `3` | Translator の 1 パス内で各ユニットに追加できる要求回数 |
| `retry_delay` | 非負整数 (`u64`) | `2` | 再試行前の待機秒数 |
| `batch_chars` | 非負整数 (`usize`) | `2500` | 1 バッチの原文文字数予算。トークン数やモデルの context window ではない |
| `max_context_items` | 非負整数 (`usize`) | `6` | 1 バッチのユニット数上限。実効値は最低 1。名前に反して隣文だけの件数ではない |
| `repair_rounds` | 非負整数 (`usize`) | `2` | 翻訳後の追加修復パス上限。`0` なら translate/run の追加パスを無効化 |
| `context_chars` | 非負整数 (`usize`) | `1200` | 1 要求の前後文 context の合計文字予算。ラベル込み。`0` は隣文の注入を無効化 |

`batch_chars` とユニット上限の先に達した条件で分割し、別ファイル・別 scene を混ぜないようにします。単一ユニットが文字予算より大きい場合、予算はそのユニットを細切れにする保証ではありません。要求には system prompt、保護トークン、語彙表、メモ、有限の前後原文とコミット済み訳文も含まれるため、実際の入力は原文予算より大きくなります。

1 パスで各ユニットに最大 `1 + retry_count` 回要求します。初回翻訳に加えて最大 `repair_rounds` パスがあり、`repair` コマンド自身は最大 `max(1, repair_rounds)` パスを試します。進展がなくなれば早く停止できます。上限は実際の HTTP 総数をそのまま表すものではなく、バッチ共有・分割で件数が変わります。

ネットワーク障害、408、429、5xx は有限再試行、モデルの ID や品質の不一致は失敗項目だけを小さいバッチで再試行します。400、401、403 など恒久的 4xx は要求を止め、新規に待機中の要求を抑制します。すでに送った要求は完了し得ます。キー rotation はありません。

## `[glossary]`

| キー | 型 | 既定値 | 効果 |
|---|---|---|---|
| `enabled` | 真偽値 | `false` | run が抽出後に有料の語彙表構築を行うか |
| `min_occurrences` | 非負整数 (`usize`) | `10` | 実原文でその語を含むユニット数の閾値。複数行ユニットも 1 件。実効値は最低 1。姓名框の名前には専用扱いがある |
| `max_terms` | 非負整数 (`usize`) | `200` | 高頻度順で保持する語数上限。実効値は最低 1 |
| `inject_limit` | 非負整数 (`usize`) | `30` | 翻訳バッチに入れる語数上限。その原文に実際に出る語を選ぶ。`0` は注入しない |

明示的な `glossary build` と `run --glossary` は enabled=false でも実行します。`run --no-glossary` はその実行で無効にします。両 run フラグは併用できません。抽出要求は最大 40 バッチに制限され、すべての原文を読む保証はありません。報告の `truncated` は max_terms によって落とした候補数で、40 バッチの原文 coverage の不足を表す値ではありません。通常の構築エラーは run に記録して続けられますが、致命的認証・設定エラーはそこで全体を止め、残りの語彙バッチを送信しません。語彙一致は意味や語形変化まで保証しません。

## `[learn]`

| キー | 型 | 既定値 | 効果 |
|---|---|---|---|
| `auto_summarize` | 真偽値 | `true` | 成功した writeback 後に既存証拠から経験をまとめる。通常は API を呼ばない |
| `llm_review` | 真偽値 | `false` | 提案を追加のモデル要求で審査する。有料 |

`writeback --no-learn` はその呼び出しの自動まとめを省略します。`learn summarize --llm` は有料審査を明示的に要求します。学習ルール、作品内上書き、承認条件は[使い方](usage.md)にあります。

## 秘密情報と費用

キーはローカル設定に置き、リポジトリ、prompt、JSONL、スクリーンショット、チャットに入れません。attx の設定は平文です。gitignore はアクセス制御や、既に追跡されたファイルの秘密除去にはなりません。POSIX なら `chmod 600 setting.toml`、Windows なら適切なファイル ACL を使います。API キーを環境変数から展開する専用設定構文はありません。`ATTX_HOME` はキーの値ではなく保存場所を指定します。

明示的に認証不要のローカルサービスでは api_key フィールドを空文字列にし、dummy Key を作りません。設定キーを含むモデル本文は拒否し、HTTP error に一致する秘密文字列は伏せます。原文や設定全体を公開してよいという意味ではありません。

有料になり得る操作は `doctor --ping`、翻訳、再試行、修復、語彙構築、プロファイル推論、LLM 学習審査です。`detect`、`analyze`、抽出、status/review、通常の経験まとめ、対応する dry-run はモデルを呼びません。価格表・実測 tokens がなければ金額を正確に見積もれません。

429 が続く場合は `worker_count` と `rpm` を下げ、切断なら出力上限・バッチ規模・timeout を見直します。retry や repair 上限を無条件に増やして費用を消費するより、[トラブルシューティング](troubleshooting.md)で原因を切り分けてください。
