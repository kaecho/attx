# 安装

## 发行版二进制

从 [GitHub Releases](https://github.com/kaecho/attx/releases)（tag `v*`）下载适合你操作系统的压缩包。可用目标：Linux x86_64、Windows x86_64、macOS x86_64 + aarch64。把 `attx` / `attx.exe` 放进你的 `PATH`。

## 从源码构建

```bash
git clone https://github.com/kaecho/attx.git
cd attx
cargo build --release
./target/release/attx --help
cargo install --path .   # optional
```

需要较新的 stable Rust（edition 2024）。不使用 nightly 特性，不锁定 MSRV。

## LLM 配置 —— 两条路径

### A. Agent Q&A（推荐）

安装 Skill，然后让 agent 配置 attx。它会依次走端点 → Key → 模型 → 语言，并写入 `setting.toml` 而不回显 Key。见 [Agent](agents.md)。

### B. 手动

```bash
cp setting.example.toml setting.toml
```

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"          # OpenAI-compatible Chat Completions
base_url = "https://api.example.com/v1"
api_key = "YOUR_API_KEY"
model = "your-model"
timeout = 600                     # seconds, per request
# temperature = 0.3               # 省略则翻译 0.3，glossary/learn JSON 0.0
# reasoning_effort = "medium"     # 省略则不发送
# max_tokens = 8192               # 省略则不发送
# stream = true                   # 省略则 false；按 SSE delta.content 拼接
# extra = { top_p = 0.9 }         # 最后合并进请求体；不能替换 messages

[translation]
worker_count = 8       # parallel HTTP batches
rpm = 60               # global rate limit per minute (0 = unlimited)
retry_count = 3
retry_delay = 2
batch_chars = 2500     # max source chars per batch
max_context_items = 6  # max units per batch
```

然后验证：

```bash
attx doctor --ping
```

`doctor` 检查配置，列出内置适配器与已保存的 Profile；`--ping` 还会向 LLM 发送一次极小的请求。机器可读形式：`attx doctor --json`。

### 配置查找顺序

`--config <path>` → `$ATTX_HOME/setting.toml` → `./setting.toml`。

- `ATTX_HOME` 也存放你保存的 Profile 与学到的经验：`$ATTX_HOME/profiles/`、`$ATTX_HOME/knowledge/`。
- 未设置 `ATTX_HOME` 时使用平台配置目录（Linux 上为 `~/.config/attx/`）。
- `setting.toml` 已被 gitignore —— **绝不提交 API Key**。
- `--client <name>` 可在单次调用中切换到非默认的 `[[llm.clients]]` 条目。

## 验证安装

```bash
attx formats                 # list of built-in adapters as JSON
attx detect --input <file>   # which adapter claims your input
attx --help
```
