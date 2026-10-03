# Teleaf 安装与发布

仓库名和命令名使用 **teleaf**。名称结合 Telegram 与 leaf，强调轻量。Cargo 包、TUI 标题和安装命令已使用新名；账号数据目录保留 `tg-tui`，兼容已有登录。

## 发布流程

1. 使用 `YoisakiKnd/teleaf` 仓库，将当前源码连同 `.github/`、`packaging/` 和 `scripts/` 推送到默认分支。`target/`、本地账号数据和 `.env` 不提交。
2. 在 Actions 页面手动运行 **Release packages**，先检查五个平台能否构建和加载 TDLib。手动运行只生成 Actions artifacts，不发布 Release 或修改包清单。
3. 确认 `Cargo.toml` 的版本后，推送对应标签，例如 `v0.1.0`。标签必须与 Cargo 版本完全一致。
4. 标签触发构建、运行库打包和加载检查；全部成功后创建 GitHub Release，发布各平台压缩包、SHA-256、Homebrew 配方和 Scoop 清单。
5. 发布任务将生成的 `Formula/teleaf.rb` 提交到主仓库默认分支；主仓库仍充当显式 URL 的 Homebrew tap。独立的 [scoop-teleaf](https://github.com/YoisakiKnd/scoop-teleaf) 仓库每小时读取主仓库的最新稳定 Release，核对 Windows 安装包 URL、`SHA256SUMS` 和 GitHub 资产哈希，再将 Release 附带的 `teleaf.json` 提交为 `bucket/teleaf.json`。Scoop 用户无需克隆主项目源码。

两个仓库分别使用自己的 `GITHUB_TOKEN` 写入本仓库，无需个人访问令牌或跨仓库推送权限。默认分支需要允许 Actions 提交清单；如果分支保护拒绝自动推送，Release 仍已生成，可以将附带的 `teleaf.rb` 提交到主仓库的 `Formula/`，将 `teleaf.json` 提交到 Scoop 仓库的 `bucket/`。

GitHub 定时任务可能延迟，长期无活动的公开仓库也可能被停用定时任务。需要立即同步或重新启用时，在 Scoop 仓库的 **Actions → Sync Teleaf release → Run workflow** 手动运行。同步失败会保留上一次的清单，不覆盖为未经校验的数据。

带 `-` 的版本标签作为 prerelease 发布，不更新默认分支上的稳定安装清单。包清单和 SHA256SUMS 始终从该次实际构建产物生成，源码中不写虚假的哈希或尚不存在的仓库 URL。

## Scoop 双仓库发布约定

每次稳定版本发布都必须向以下两个仓库提交同一份清单：

1. `YoisakiKnd/scoop-teleaf`：同步 `bucket/teleaf.json` 并确认提交成功。
2. `Mythos-404/eimer`：向默认分支提交 `bucket/teleaf.json` 的更新 PR；已有同一版本的开放 PR 时更新该 PR，避免重复提交。

两个仓库的版本、下载地址、SHA-256 和许可证必须一致。eimer 的合并由其维护者决定；发布记录必须包含更新 PR 链接，未合并时不得宣称该版本已可从 eimer 安装。这项持续约定同时保存在 `AGENTS.md`，后续发布任务必须执行。

自有 bucket 自动同步；eimer 的更新 PR 使用发布者本机已登录的 GitHub CLI 提交。仓库内的 `GITHUB_TOKEN` 没有跨仓库推送权限，因此当前并未配置无人值守的跨仓库 PR 创建。需要该功能时可另行配置专用 GitHub App 或访问令牌。

项目许可证为 MIT，清单使用 `license: MIT`。第三方运行库保留自己的许可文本。新的发布包自动包含根目录 `LICENSE`；`v0.1.0` 的既有安装包保持原始哈希，MIT 文本作为额外 Release 资产提供。

## 用户安装

[v0.1.0](https://github.com/YoisakiKnd/teleaf/releases/tag/v0.1.0)及安装清单已发布：

```sh
brew tap YoisakiKnd/teleaf https://github.com/YoisakiKnd/teleaf
brew install YoisakiKnd/teleaf/teleaf
teleaf --check
```

```powershell
scoop bucket add teleaf https://github.com/YoisakiKnd/scoop-teleaf
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

五个平台已通过[原生 CI 验证](https://github.com/YoisakiKnd/teleaf/actions/runs/37094100770)：97 项 Rust 单元测试、7 项打包测试、Clippy、Release 构建，以及暂存发布包的 `teleaf --check` 加载检查。Windows 初次打包因 Python CP1252 无法输出中文失败，已在脚本中显式使用 UTF-8，并加入旧编码环境下的下载和错误提示回归测试。

本地还验证了 macOS ARM64 归档、依赖重定位、解压后加载及模拟 Homebrew 的符号链接路径；正式发布的 macOS ARM64 包约 11.62 MiB，Homebrew 下载后的 SHA-256 和独立解压加载检查通过。更名后的离线快捷消息 PTY 回归通过，`--help` / `--version` / `--check` 未创建账号数据。

Homebrew 的 tap、配方解析、下载和校验通过，本机实际安装被 Homebrew 的 Command Line Tools 版本检查阻止，尚未完成 `brew test`。遇到同样的提示，请按 Homebrew 提示更新系统开发工具，或直接下载 Release 解压运行。Scoop 实际安装及 Windows 10 实机完整交互尚未验收；Windows CI 已验证发布包包含所需 DLL 并能加载 TDLib。

参照：[Homebrew tap](https://docs.brew.sh/Taps)、[Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)、[Scoop manifest](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests)。
