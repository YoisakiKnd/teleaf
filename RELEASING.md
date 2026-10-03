# Teleaf 安装与发布

仓库名和命令名使用 **teleaf**。名称结合 Telegram 与 leaf，强调轻量。Cargo 包、TUI 标题和安装命令已使用新名；账号数据目录保留 `tg-tui`，兼容已有登录。

## 创建仓库后

1. 使用 `YoisakiKnd/teleaf` 仓库，将当前源码连同 `.github/`、`packaging/` 和 `scripts/` 推送到默认分支。`target/`、本地账号数据和 `.env` 不提交。
2. 在 Actions 页面手动运行 **Release packages**，先检查五个平台能否构建和加载 TDLib。手动运行只生成 Actions artifacts，不发布 Release 或修改包清单。
3. 确认 `Cargo.toml` 的版本后，推送对应标签，例如 `v0.1.0`。标签必须与 Cargo 版本完全一致。
4. 标签触发构建、运行库打包和加载检查；全部成功后创建 GitHub Release，发布各平台压缩包、SHA-256、Homebrew 配方和 Scoop 清单。
5. 发布任务将生成的 `Formula/teleaf.rb` 和 `bucket/teleaf.json` 提交到默认分支。同一个仓库同时充当显式 URL 的 Homebrew tap 和 Scoop bucket，无需另外创建两个仓库。

常规使用仓库内的 `GITHUB_TOKEN`，无需个人访问令牌。默认分支需要允许 Actions 提交这两份清单；如果分支保护拒绝自动推送，Release 仍已生成，可以将 Release 附带的 `teleaf.rb` 和 `teleaf.json` 分别提交到 `Formula/` 和 `bucket/` 后再使用包管理器安装。

带 `-` 的版本标签作为 prerelease 发布，不更新默认分支上的稳定安装清单。包清单和 SHA256SUMS 始终从该次实际构建产物生成，源码中不写虚假的哈希或尚不存在的仓库 URL。

## 用户安装

首个 Release 和清单发布后：

```sh
brew tap YoisakiKnd/teleaf https://github.com/YoisakiKnd/teleaf
brew install YoisakiKnd/teleaf/teleaf
teleaf --check
```

```powershell
scoop bucket add teleaf https://github.com/YoisakiKnd/teleaf
scoop install teleaf/teleaf
teleaf --check
```

这是项目自己的安装源；尚未加入 Homebrew core 或 Scoop main。升级：`brew upgrade teleaf`；Windows 执行 `scoop update`、`scoop update teleaf`。账号配置放在程序目录之外，不被包管理器升级替换。

## 发布包结构

```text
teleaf                       # Windows 为 teleaf.exe
*.dll                        # 仅 Windows，随附 MSVC 可再分发运行库
 tdlib/
   libtdjson.dylib / libtdjson.so / tdjson.dll
   OpenSSL、zlib、libc++ 等所需运行库
 LICENSES/
 README.md
```

- macOS 15+：ARM64、x64。运行库依赖改为 `@loader_path`，使用临时代码签名；尚未提供 Developer ID 签名和 Apple 公证。
- Linux：Ubuntu 24.04 的 glibc 基线，x64、ARM64；随附非系统依赖并写入 `$ORIGIN`，glibc 和系统加载器由系统提供。不是 musl/Alpine 通用包。
- Windows 10/11：x64。随附 TDLib 的 DLL 依赖和 MSVC 可再分发运行库；从 DLL 所在目录加载依赖，不要求用户设置 PATH。

TDLib 上游发布包固定为 tdlib-rs `v1.4.0` 中的 TDLib `1.8.61`，平台和 SHA-256 记录在 `packaging/tdlib.json`。运行库许可证随包附带；项目自己的 `LICENSE` 存在时也会附带。更新 TDLib 时需同时更新版本、平台哈希、运行库依赖及加载测试。

## 本地打包验证

在相同系统和架构的构建机运行，不用跨编译的可执行文件代替本机加载检查：

```sh
cargo build --release --locked
python3 scripts/package-release.py --binary target/release/teleaf --version 0.1.0
python3 scripts/test-packaging.py
```

Windows 的 binary 参数为 `target/release/teleaf.exe`，打包机需要 Visual Studio C++ 的 MSVC Redist 目录。Linux 打包机需安装 `patchelf`、`libc++1-18`、`libc++abi1-18` 和 `libssl3t64`。macOS 打包机需要 `openssl@3`、`otool`、`install_name_tool` 和 `codesign`。

`package-release.py` 下载校验过的 TDLib、复制运行库、调整动态加载路径，再从项目目录外运行暂存包的 `teleaf --check`。检查失败则不生成归档。归档不包含 `config.json`、API 凭据、聊天数据库、下载缓存或编译产物目录。

五个平台的产物齐全后，可以独立生成包清单：

```sh
python3 scripts/generate-packages.py --repository YoisakiKnd/teleaf --version 0.1.0 --assets target/dist --output target/packages
```

输出真实配方、Scoop 清单和 SHA256SUMS。该命令拒绝缺失的平台产物和不合法的仓库/版本输入；生成器只写本地文件，不创建远程仓库或发布内容。

## 验证范围

本地已验证 macOS ARM64 归档、依赖重定位、解压后加载及模拟 Homebrew 的符号链接路径，归档约 11.43 MiB。97 项 Rust 单元测试、5 项打包测试、Clippy、Windows 编译检查及更名后的离线快捷消息 PTY 回归通过。`--help` / `--version` / `--check` 未创建账号数据。Linux/Windows 的实际运行库加载、远程 Homebrew/Scoop 安装及 GitHub 发布流程尚待仓库创建后的 CI 验证。

参照：[Homebrew tap](https://docs.brew.sh/Taps)、[Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)、[Scoop manifest](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests)。
