# attx

[English](README.md) | **中文**

**Agent Translation Toolkit eXtensible（可扩展的 Agent 翻译工具包）** — 纯 Rust 实现的通用游戏文本翻译框架，面向 AI Agent 与人工流程。

```
提取 extract（引擎适配器） → 翻译 translate（LLM 核心） → 写回 writeback（引擎适配器）
```

| 适配器 | 目标 |
|--------|------|
| `rmmz` | RPG Maker MV / MZ（`data/*.json` 对白、System、基础数据库） |
| `jsonl` | 通用 JSONL 文本包（任意引擎，用外部脚本提取/写回） |

设计目标对齐 [att-mz#11](https://github.com/yexi-by/att-mz/issues/11) 的通用化方向。

---

## 安装

### 发行包

从 [Releases](https://github.com/emptysuns/attx/releases) 下载（标签 `v*`）。

### 源码编译

```bash
git clone https://github.com/emptysuns/attx.git
cd attx
cargo build --release
./target/release/attx --help
# 可选安装到 PATH：
cargo install --path .
```

---

## 配置大模型

```bash
cp setting.example.toml setting.toml
```

编辑 `setting.toml`：

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"
base_url = "https://你的服务商地址/v1"
api_key = "你的API_KEY"
model = "模型名称"
timeout = 600

[translation]
worker_count = 8
rpm = 60
retry_count = 3
retry_delay = 2
batch_chars = 2500
max_context_items = 6
```

说明：

- `provider_type` 目前固定为 `openai`（OpenAI 兼容 Chat Completions）。
- `base_url` 一般以 `/v1` 结尾。
- `setting.toml` 已在 `.gitignore` 中，**不要把 API Key 提交进仓库**。

连通性检查：

```bash
attx doctor --ping
```

---

## 如何用 attx 翻译游戏（RPG Maker MV/MZ）

### 1. 识别引擎

```bash
attx detect --game /path/to/game
# → {"engine":"rmmz","content_root":"...","label":"RPG Maker MV/MZ"}
```

### 2. 创建工作区

```bash
attx init --game /path/to/game --src ja --dst zh
# 默认工作区：/path/to/game/.attx
# 或自定义：
attx init --game /path/to/game --src ja --dst zh --workspace /tmp/my-game-ws
```

- `--src`：原文语言（`ja` 或 `en`）
- `--dst`：目标语言（当前提示词面向简体中文）

### 3. 提取文本

```bash
attx extract --workspace /path/to/game/.attx
attx status --workspace /path/to/game/.attx
```

会提取事件对白（指令 101/401/102/405）、`System.json` 系统词，以及基础库字段（Actors、Items 等）。

### 4. 调用模型翻译

```bash
# 翻译全部 pending
attx translate --workspace /path/to/game/.attx

# 先小批量试跑
attx translate --workspace /path/to/game/.attx --limit 20

# 只看计划、不真正请求模型
attx translate --workspace /path/to/game/.attx --dry-run
```

相同原文 hash 会缓存在工作区数据库里，重复运行会跳过已翻译条目。

### 5. 写回游戏文件

```bash
# 预览会改哪些文件
attx writeback --workspace /path/to/game/.attx --dry-run

# 真正写回（每个被改文件旁保留一份 *.attxbak 备份）
attx writeback --workspace /path/to/game/.attx
```

然后启动游戏试玩检查。

### 一条命令跑通

```bash
attx run --game /path/to/game --src ja --dst zh
# 可选：
#   --limit N
#   --no-translate
#   --no-writeback
#   --workspace /自定义目录
```

### 手工 / 离线 JSONL 流程

适合审校、外部工具，或非 RM 引擎：

```bash
attx export-jsonl --workspace .attx --output pending.jsonl --filter pending
# 在外部翻译 pending.jsonl 后：
attx import-jsonl --workspace .attx --input translated.jsonl
attx writeback --workspace .attx
```

纯文本管道（不依赖游戏目录）：

```bash
# 每行：{"id":"scene1:55","text":"…","context":"op","role":"Hero"}
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh
```

---

## 命令一览

| 命令 | 作用 |
|------|------|
| `doctor [--ping]` | 检查配置 / 可选 ping 模型 |
| `detect --game` | 探测引擎 |
| `init --game` | 创建工作区与 SQLite |
| `extract` | 适配器提取文本单元 |
| `translate` | 对 pending 调用 LLM |
| `writeback` | 适配器写回游戏文件 |
| `run` | init + extract + translate + writeback |
| `status` | 进度统计 |
| `translate-jsonl` | 纯文本翻译管道 |
| `export-jsonl` / `import-jsonl` | 导入导出 |

全局参数：`--config /path/to/setting.toml`（默认 `./setting.toml` 或 `$ATTX_HOME/setting.toml`）。

---

## 扩展新引擎

在 `src/adapter/` 实现 `EngineAdapter`，并在 `all_adapters()` 注册即可：

```rust
pub trait EngineAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn detect(&self, game_path: &Path) -> Option<DetectHit>;
    fn extract(&self, content_root: &Path, source_lang: &str) -> Result<Vec<TextUnit>>;
    fn writeback(
        &self,
        content_root: &Path,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<BTreeMap<String, String>>; // 相对路径 → 文件内容
}
```

核心管线不用改。尚未内置的引擎：外部提取成 JSONL → `translate-jsonl` → 自有脚本写回。

---

## 目录结构

```
src/
  main.rs          CLI
  model.rs         TextUnit / Translation / 控制符占位
  config.rs        setting.toml
  store.rs         SQLite 工作区
  llm.rs           OpenAI 兼容对话与分批
  quality.rs       行数 / 控制符检查
  pipeline.rs      编排
  adapter/
    mod.rs         特质与注册
    rmmz.rs        RPG Maker MV/MZ
    jsonl.rs       通用 JSONL
```

---

## 暂未包含

- 插件 JS AST / Note 标签 Agent 规则流（见 att-mz）
- RGSS Marshal / 加密归档
- Unity / Ren'Py / Godot 一等适配器

在适配器落地前请走 `jsonl` 路径。欢迎 PR。

---

## 许可证

MIT
