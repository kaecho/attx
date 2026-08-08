# 安装

## 发行包

从 [GitHub Releases](https://github.com/emptysuns/attx/releases) 下载对应系统压缩包，将 `attx` / `attx.exe` 加入 `PATH`。

## 源码

```bash
git clone https://github.com/emptysuns/attx.git
cd attx
cargo build --release
./target/release/attx --help
```

## 配置 LLM：两条路

### A. Agent 问答（推荐）

安装 Skill 后让 Agent 配置 attx。它会询问端点 → Key → 模型 → 语向，写入 `setting.toml` 且不回显 Key。见 [Agent](agents.md)。

### B. 手动

```bash
cp setting.example.toml setting.toml
# 编辑 base_url / api_key / model
attx doctor --ping
```

查找顺序：`--config` → `./setting.toml` → `$ATTX_HOME/setting.toml`。
