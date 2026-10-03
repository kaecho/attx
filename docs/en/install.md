# Installation

## Release packages

Download the package for your platform from [release v0.10.1](https://github.com/kaecho/attx/releases/tag/v0.10.1). Extract the whole archive so the example configuration, profiles, and agent skill remain available.

| Package | Target | Archive |
|---|---|---|
| `attx-linux-x86_64` | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| `attx-windows-x86_64` | `x86_64-pc-windows-msvc` | `.zip` |
| `attx-macos-aarch64` | `aarch64-apple-darwin` | `.tar.gz` |
| `attx-macos-x86_64` | `x86_64-apple-darwin` | `.tar.gz` |

The packages contain `attx` or `attx.exe`, the English and Chinese README, `CHANGELOG.md`, `LICENSE`, `setting.example.toml`, `skills/`, `profiles/`, `docs/`, `mkdocs.yml`, and `requirements-docs.txt`. Linux packages target GNU libc, not musl. No ARM Linux or 32-bit Windows package is in this matrix.

Put the binary on `PATH`, or invoke its full path. On Linux and macOS, make it executable if extraction did not preserve permissions:

```bash
chmod +x "./attx"
./attx --version
./attx --help
```

PowerShell does not run a current-directory executable by bare name:

```powershell
.\attx.exe --version
.\attx.exe --help
```

The translation runtime needs no Python, Node.js, external SQLite server, or asynchronous service. Python is needed only to build the documentation site.

## Build from source

Use stable Rust 1.89 or newer. The crate uses edition 2024 and declares `rust-version = "1.89"`.

```bash
git clone https://github.com/kaecho/attx.git
cd "attx"
cargo build --release
./target/release/attx --version
cargo install --path . --locked
```

On Windows, run the same Git and Cargo commands from a developer environment with the Rust target's linker available. Before installation, the executable is:

```powershell
.\target\release\attx.exe --version
```

`cargo install --path . --locked` installs the binary into Cargo's executable directory. It does not install the checkout's example configuration, skill, or profile examples beside that binary. Keep the checkout if you use those resources.

Build dependencies include bundled SQLite and Rustls TLS. The runtime HTTP client is blocking and uses ordinary scoped worker threads. See [development](development.md) for CI and release details.

## Configuration and a first check

From the extracted package or repository:

```bash
cp "setting.example.toml" "setting.toml"
```

PowerShell:

```powershell
Copy-Item ".\setting.example.toml" ".\setting.toml"
notepad ".\setting.toml"
```

Edit `base_url`, `api_key`, and `model` locally. Do not paste the key into chat. The example contains a placeholder endpoint and cannot translate until these fields are real. Then run:

```bash
attx --config "./setting.toml" doctor --json
attx --config "./setting.toml" doctor --ping --json
attx formats
```

`doctor --ping` sends a small paid-or-quota-consuming model request. A successful installation is separate from valid provider credentials. Check `llm.configured` and `ping` in the JSON, not only top-level `status`.

Configuration lookup is an explicit `--config` path, then an existing `$ATTX_HOME/setting.toml`, then `./setting.toml`. The platform configuration directory is used for saved profiles and knowledge, not as an implicit settings-file search location. Details are in [configuration](configuration.md).

Continue with the [quick start](quickstart.md), or install the bundled [agent skill](agents.md).