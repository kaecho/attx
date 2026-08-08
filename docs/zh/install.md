# 安装

## 发行包

从 [GitHub Releases](https://github.com/emptysuns/attx/releases)（`v*` 标签）下载对应系统压缩包，解压后将 `attx` / `attx.exe` 加入 `PATH`。

## 源码编译

```bash
git clone https://github.com/emptysuns/attx.git
cd attx
cargo build --release
./target/release/attx --help
cargo install --path .   # 可选
```

需要较新的 stable Rust。

## 配置 LLM

```bash
cp setting.example.toml setting.toml
```

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"
base_url = "https://api.example.com/v1"
api_key = "YOUR_API_KEY"
model = "your-model"
timeout = 600

[translation]
worker_count = 8
rpm = 60
batch_chars = 2500
```

`setting.toml` 已在 `.gitignore`。校验：

```bash
attx doctor --ping
```

查找顺序：`--config` → `./setting.toml` → `$ATTX_HOME/setting.toml`。
