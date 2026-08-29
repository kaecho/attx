# エージェント

attx は stdout に JSON を出力するローカル CLI です — すでにコーディングエージェントのツール面です。**Skill** が実行プロトコルです：段階的パイプライン、ハードストップ、Q&A 設定ウィザード。MCP は任意で、通常は不要です。

## Skill のインストール

```bash
# Claude Code（個人、全セッション）
mkdir -p ~/.claude/skills && cp -a skills/attx ~/.claude/skills/
# プロジェクトスコープ
mkdir -p .claude/skills && cp -a skills/attx .claude/skills/
```

他のエージェント（Cursor / Codex / OpenCode / …）：チェックアウトを保持し、次のように要求します：

```text
Strictly follow <attx-dir>/skills/attx/SKILL.md
```

ファイル：

```text
skills/attx/SKILL.md                                    # 段階、ハードストップ、ウィザード
skills/attx/references/cli-command-contract.md          # 正確な CLI + JSON コントラクト
skills/attx/references/agent-usage.md                   # セッション構造、落とし穴
skills/attx/references/custom-format-discovery.md       # 未知フォーマットのフロー
skills/attx/references/failure-recovery.md              # 症状 → 対処の表
skills/attx/references/jsonl-workflow.md                # JSONL データ交換
skills/attx/references/feedback-iteration.md            # プレイテスト後のフィードバックループ
```

## Q&A 設定ウィザード

設定がないか `doctor --ping` が失敗したときに起動します。エージェントは**一度に 1 項目ずつ**尋ねます：

1. API エンドポイント（OpenAI / DeepSeek / カスタムの `/v1` で終わる `base_url`）
2. API キー → `setting.toml` にのみ書き込み、二度と表示せず、チャット/ログ/git にも決して入れない
3. モデル名
4. ソース / ターゲット言語
5. 任意：並行数、用語集のオン/オフ（用語集はデフォルトでオフ — 追加の LLM 呼び出しがかかるため）

その後：`attx doctor --ping` → パイプラインのステージ 0。

## 段階的パイプライン

`detect` → `init` → `extract` → `status` → 試し `translate --limit 20` → 本番 `translate` → `review` → `writeback`（インプレースフォーマットでは最初にドライラン）。各段階の後、件数と次のステップを報告します。

Skill が強制するハードストップ：`doctor --ping` の 401（キー再試行のスパムはしない）、アダプターが見つからない（プロファイルツールチェーンに切り替え）、系統的な試し失敗（止まる、お金を燃やさない）、そして**ゲームディレクトリを上書きする writeback はユーザーの明示的な許可を必要とする**。

## コピペ用プロンプト

```text
Use the attx toolkit at <attx-dir>, following skills/attx/SKILL.md.

Help me set up attx if needed (Q&A: endpoint, key, model, languages),
then translate <input path> from Japanese into Simplified Chinese.

Rules:
1. Only the attx CLI; never hand-edit inputs, attx.db, or tool source.
2. Never print my API key.
3. doctor --ping → detect → init → extract → status → translate --limit 20 → full.
4. Prefer translated copies; ask before any in-place overwrite.
5. Report counts after each stage.
```

短縮形：

```text
Help me set up attx, then translate ./novel.epub from Japanese to Simplified Chinese.
```

## エージェントの黄金律

- **stdout の JSON が結果です。** stderr の `batch i/n` 進捗行は結果ではありません。
- **入力を手で編集しない** — `attx.db` もツールのソースも — CLI を通して操作します。
- **API キーをエコーしない** — `setting.toml` にのみ存在します。
- `learn review --approve-all` はエージェントに禁止：`skip` エントリはテキストを削除するため、保留エントリを証拠付きで報告し、ユーザーに決定させること。
- 締めくくり時に `status.passthrough > 0` は報告しなければなりません；`--retry-passthrough` でそれらのユニットを再キューします。
- 未知のフォーマット → `analyze` → `profile new`/`test` → `init --profile`（`detect` の失敗で決して諦めない）。
