# 安装

## 下载发行包

从 [GitHub Releases](https://github.com/kaecho/attx/releases) 下载 `v0.10.0` 的对应压缩包。

| 平台 | 构建目标 | 包名 |
|------|----------|------|
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `attx-linux-x86_64.tar.gz` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `attx-windows-x86_64.zip` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `attx-macos-aarch64.tar.gz` |
| macOS Intel | `x86_64-apple-darwin` | `attx-macos-x86_64.tar.gz` |

解压后保留 `setting.example.toml`、`skills/`、`profiles/`、`docs/` 及文档构建配置。包另含 README、CHANGELOG 和 LICENSE。把 `attx` 或 `attx.exe` 放入 PATH，也可以始终使用完整路径。Linux 包不是 musl 静态包；没有 ARM Linux 或 32 位 Windows 的预构建包。

Linux/macOS 在解压目录运行：

```bash
./attx --version
./attx --help
```

Windows PowerShell 在解压目录运行：

```powershell
.\attx.exe --version
.\attx.exe --help
# 可执行文件路径含空格时使用调用运算符
& 'C:\Tools\attx toolkit\attx.exe' --help
```

不要把终端或系统拦截报错直接当成工具故障。先确认包架构、执行权限和路径，再按系统的正常安全流程处理。

## 从源码构建

需要 Rust 1.89 或更新的 stable 工具链，使用 Rust 2024 edition。不需要 nightly。SQLite 随依赖构建；HTTP 使用 rustls，模型请求为同步调用。

```bash
git clone https://github.com/kaecho/attx.git
cd attx
cargo build --release
./target/release/attx --help
# 可选：安装到 Cargo 的 bin 目录
cargo install --path .
```

Windows 对应二进制为 `.\target\release\attx.exe`，MSVC 工具链需要其正常的编译环境。二次开发和发布矩阵见[开发页](development.md)。

## 配置不是安装的一部分

离线命令不要求有效模型配置：

```bash
attx formats
attx detect --input './novel.epub'
```

真正翻译前复制示例，在本机编辑 `base_url`、`api_key` 和 `model`：

```bash
cp setting.example.toml setting.toml
attx --config './setting.toml' doctor --json --ping
```

PowerShell：

```powershell
Copy-Item '.\setting.example.toml' '.\setting.toml'
.\attx.exe --config '.\setting.toml' doctor --json --ping
```

`--ping` 会产生一次很小的模型请求。检查 `llm.configured` 和 `ping`；`doctor` 顶层 `status="ok"` 或退出码 0 不表示接口连接成功。

配置查找只包含显式 `--config`、存在的 `$ATTX_HOME/setting.toml`、当前目录 `setting.toml`。平台用户配置目录用于 Profile/经验，不会自动提供模型配置。密钥不要写进命令行、聊天、工单或版本库。完整字段与环境变量作用域见[配置参考](configuration.md)。

下一步：[快速开始](quickstart.md) · [Agent 安装](agents.md)。
