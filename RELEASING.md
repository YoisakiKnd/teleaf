# Teleaf 安装与发布

仓库名和命令名使用 **teleaf**。名称结合 Telegram 与 leaf，强调轻量。Cargo 包、TUI 标题和安装命令已使用新名；账号数据目录保留 `tg-tui`，兼容已有登录。

## 发布流程

### 项目 API 凭据

在 Telegram 为 **Teleaf** 注册一个自己的应用，由发布者提供 API ID / Hash，使用安装包的用户无需逐个创建应用。在主仓库 **Settings → Secrets and variables → Actions → New repository secret** 设置：

- `TELEAF_APP_API_ID`：应用的正整数 API ID。
- `TELEAF_APP_API_HASH`：配套的 32 位十六进制 API Hash。

五个平台的 Release 工作流会将这组凭据编译进安装包。标签发布时必须配置两项 Secrets，缺少任意一项会在构建前停止，以免发行仍要求用户注册应用的安装包；手动打包验证和源码构建允许不配置。首次运行自动生成本地配置并进入登录，用户仍需本人手机号、验证码和两步验证密码。Secrets 避免在源码中明文提交，但随客户端分发的应用凭据可从二进制提取，不应当作账号密码、Bot Token 或登录会话使用。

源码构建可在构建环境设置同名变量；无项目凭据时保留手动表单。只填写一个变量或格式错误会使构建失败，错误信息不包含实际凭据。运行时读取优先级为：已有 `config.json` → `TG_API_ID` / `TG_API_HASH` → 安装包项目凭据；每组必须来自同一来源，不混用 ID / Hash。已有账号升级后继续使用已保存的凭据和数据库密钥。

Telegram 官网注册错误仍需在官网解决；已有应用无需再次创建。不能使用官方代码中的受限示例凭据代替 Teleaf 自己的应用。参见 [创建应用](https://core.telegram.org/api/obtaining_api_id) 和 [API 条款](https://core.telegram.org/api/terms)。

### 打包与同步

1. 使用 `YoisakiKnd/teleaf` 仓库，将当前源码连同 `.github/`、`packaging/` 和 `scripts/` 推送到默认分支。`target/`、本地账号数据和 `.env` 不提交。
2. 在 Actions 页面手动运行 **Release packages**，先检查五个平台能否构建和加载 TDLib。手动运行只生成 Actions artifacts，不发布 Release 或修改包清单。
3. 确认 `Cargo.toml` 的版本后，推送对应标签，例如 `v0.1.1`。标签必须与 Cargo 版本完全一致。
4. 标签触发构建、运行库打包和加载检查；全部成功后创建 GitHub Release，发布各平台压缩包、SHA-256、Homebrew 配方和 Scoop 清单。
5. 发布任务将生成的 `Formula/teleaf.rb` 提交到主仓库默认分支；主仓库仍充当显式 URL 的 Homebrew tap。独立的 [scoop-teleaf](https://github.com/YoisakiKnd/scoop-teleaf) 仓库每小时读取主仓库的最新稳定 Release，核对 Windows 安装包 URL、`SHA256SUMS` 和 GitHub 资产哈希，再将 Release 附带的 `teleaf.json` 提交为 `bucket/teleaf.json`。Scoop 用户无需克隆主项目源码。

两个仓库分别使用自己的 `GITHUB_TOKEN` 写入本仓库，无需个人访问令牌或跨仓库推送权限。默认分支需要允许 Actions 提交清单；如果分支保护拒绝自动推送，Release 仍已生成，可以将附带的 `teleaf.rb` 提交到主仓库的 `Formula/`，将 `teleaf.json` 提交到 Scoop 仓库的 `bucket/`。

GitHub 定时任务可能延迟，长期无活动的公开仓库也可能被停用定时任务。需要立即同步或重新启用时，在 Scoop 仓库的 **Actions → Sync Teleaf release → Run workflow** 手动运行。同步失败会保留上一次的清单，不覆盖为未经校验的数据。

带 `-` 的版本标签作为 prerelease 发布，不更新默认分支上的稳定安装清单。包清单和 SHA256SUMS 始终从该次实际构建产物生成，源码中不写虚假的哈希或尚不存在的仓库 URL。

## Scoop 发布约定

2026-10-04 起按用户的新要求，稳定版本只推送主仓库 `YoisakiKnd/teleaf` 并更新 `YoisakiKnd/scoop-teleaf` 的 `bucket/teleaf.json`，不再为 `Mythos-404/eimer` 创建或更新 PR。已提交的历史 PR 保留，由维护者自行处理。这项约定同时保存在 `AGENTS.md`。

Scoop 清单必须与该次实际发布的 Windows ZIP 版本、下载地址、SHA-256 和 MIT 许可证一致，核验成功后再同步。

`v0.1.0` 首次收录 PR：[Mythos-404/eimer#1](https://github.com/Mythos-404/eimer/pull/1)。

`v0.1.1` 已发布并同步自有 bucket（提交 `8bbe829`）；eimer 更新 PR：[Mythos-404/eimer#2](https://github.com/Mythos-404/eimer/pull/2)，提交时等待维护者合并。两份清单与 Release 的 `teleaf.json` 相同，Windows ZIP 的 SHA-256 为 `6b390f50c6da83fcc76323add9ea2b3c83eac1d56c82e86a53c9a440b0cb411a`。Homebrew 配方由发布工作流同步（提交 `f4302e0`）。

自有 bucket 自动同步，必要时可手动触发同步工作流；本机 GitHub CLI 可核验清单和同步结果。

项目许可证为 MIT，清单使用 `license: MIT`。第三方运行库保留自己的许可文本。新的发布包自动包含根目录 `LICENSE`；`v0.1.0` 的既有安装包保持原始哈希，MIT 文本作为额外 Release 资产提供。

## 用户安装

[v0.1.1](https://github.com/YoisakiKnd/teleaf/releases/tag/v0.1.1)及安装清单已发布：

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
 LICENSE
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
python3 scripts/package-release.py --binary target/release/teleaf --version 0.1.1
python3 scripts/test-packaging.py
```

Windows 的 binary 参数为 `target/release/teleaf.exe`，打包机需要 Visual Studio C++ 的 MSVC Redist 目录。Linux 打包机需安装 `patchelf`、`libc++1-18`、`libc++abi1-18` 和 `libssl3t64`。macOS 打包机需要 `openssl@3`、`otool`、`install_name_tool` 和 `codesign`。

`package-release.py` 下载校验过的 TDLib、复制运行库、调整动态加载路径，再从项目目录外运行暂存包的 `teleaf --check`。检查失败则不生成归档。归档不包含 `config.json`、API 凭据、聊天数据库、下载缓存或编译产物目录。

五个平台的产物齐全后，可以独立生成包清单：

```sh
python3 scripts/generate-packages.py --repository YoisakiKnd/teleaf --version 0.1.1 --assets target/dist --output target/packages
```

输出真实配方、Scoop 清单和 SHA256SUMS。该命令拒绝缺失的平台产物和不合法的仓库/版本输入；生成器只写本地文件，不创建远程仓库或发布内容。

## 验证范围

`v0.1.1` 五个平台已通过[原生 CI 验证](https://github.com/YoisakiKnd/teleaf/actions/runs/37119341960)：101 项 Rust 单元测试、7 项打包测试、Clippy、Release 构建，以及暂存发布包的 `teleaf --check` 加载检查。Intel CI 的离线测试在生成 PNG 时触发一秒等待超时，改为有界的总截止时间后五平台通过；全部功能断言保留。Windows 首次发布时因 Python CP1252 无法输出中文失败，此前已显式使用 UTF-8，并加入旧编码环境下的下载和错误提示回归测试。

已重新下载 `v0.1.1` 全部五个平台的实际归档，逐一核对 `SHA256SUMS`、GitHub 资产摘要、主程序、TDLib、项目 MIT LICENSE 和第三方许可；Scoop 清单与 Homebrew 配方的版本、URL 和哈希全部一致。macOS ARM64 发布包在项目目录外独立解压，`--version` 返回 `Teleaf 0.1.1`，`--check` 加载随附 TDLib 1.8.61，未创建账号配置。

本地还验证了 macOS ARM64 归档、依赖重定位、解压后加载及模拟 Homebrew 的符号链接路径；正式发布的 macOS ARM64 包约 11.62 MiB，Homebrew 下载后的 SHA-256 和独立解压加载检查通过。更名后的离线快捷消息 PTY 回归通过，`--help` / `--version` / `--check` 未创建账号数据。

Homebrew 的 tap、配方解析、下载和校验通过，本机实际安装被 Homebrew 的 Command Line Tools 版本检查阻止，尚未完成 `brew test`。遇到同样的提示，请按 Homebrew 提示更新系统开发工具，或直接下载 Release 解压运行。Scoop 实际安装及 Windows 10 实机完整交互尚未验收；Windows CI 已验证发布包包含所需 DLL 并能加载 TDLib。

参照：[Homebrew tap](https://docs.brew.sh/Taps)、[Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)、[Scoop manifest](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests)。
