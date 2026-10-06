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
3. 确认 `Cargo.toml` 的版本后，推送对应标签，例如 `v0.1.5`。标签必须与 Cargo 版本完全一致。
4. 标签触发构建、运行库打包和加载检查；全部成功后创建 GitHub Release，发布各平台压缩包、SHA-256、Homebrew 配方和 Scoop 清单。
5. 发布任务将生成的 `Formula/teleaf.rb` 提交到主仓库默认分支；主仓库仍充当显式 URL 的 Homebrew tap。独立的 [scoop-bucket](https://github.com/YoisakiKnd/scoop-bucket) 仓库每小时读取主仓库的最新稳定 Release，核对 Windows 安装包 URL、`SHA256SUMS` 和 GitHub 资产哈希，再将 Release 附带的 `teleaf.json` 提交为 `bucket/teleaf.json`。Scoop 用户无需克隆主项目源码。

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

## 0.1.2 发布验证

2026-10-04 已发布 [v0.1.2](https://github.com/YoisakiKnd/teleaf/releases/tag/v0.1.2)，标签对应 `1b3d1ec`。主仓库原生 CI [37167271185](https://github.com/YoisakiKnd/teleaf/actions/runs/37167271185) 通过：Unix 105 / Windows 106 项单元测试，Windows ConPTY 登录、关闭鼠标后的输入、窗口调整和退出，以及 Clippy 与终端回归。

正式发布 [37167430406](https://github.com/YoisakiKnd/teleaf/actions/runs/37167430406) 五个平台均通过原生构建、测试、运行库加载和打包；发布前运行实际 Linux x64 安装包验证内置 API 凭据能进入手机号页面且保留已有配置。下载后的五份安装包、SHA256SUMS、GitHub 资产摘要及许可证均已核验；实际 macOS ARM 安装包也通过相同登录启动验证。测试未提交真实手机号或验证码。

Homebrew 配方更新至 0.1.2（`bbf5f73`）；自有 Scoop [同步任务 37167698846](https://github.com/YoisakiKnd/scoop-teleaf/actions/runs/37167698846) 成功（`3da39a1`）。两份包清单与 Release 资产一致。Windows ZIP SHA-256：`e8e7e83787ca3481af73ef83bd0ffc284db9853cf6041938f935a3bb2e7ec63d`。按新约定，没有为 eimer 创建或更新 PR。

Windows Terminal 的实际 GPU 图片显示仍需终端实机验证；ConPTY 原生输入和 Sixel 编码/协议测试不能代替该项。

## 0.1.3 发布验证

2026-10-06（北京时间）已发布 [v0.1.3](https://github.com/YoisakiKnd/teleaf/releases/tag/v0.1.3)，标签对应 `3ade5bedda203e221f0cf39b93ab069d9751ada4`。包含发送后保留输入焦点、Windows 原生通知、按需读取剪贴板图片/文件、侧栏箭头、媒体文字回退和搜索/弹窗操作修复，详见 `CHANGELOG.md`。

[主仓库 CI 37349909101](https://github.com/YoisakiKnd/teleaf/actions/runs/37349909101) 与 [标签 CI 37349913624](https://github.com/YoisakiKnd/teleaf/actions/runs/37349913624) 均通过，包含 Linux 终端回归、剪贴板入口和 Windows ConPTY 输入。[正式发布 37349913664](https://github.com/YoisakiKnd/teleaf/actions/runs/37349913664) 五个平台全部通过：Linux 各 124 项、macOS 各 125 项、Windows 126 项单元测试，以及 Clippy、打包检查和 TDLib 运行库加载。macOS 测试使用私有剪贴板验证图片、文件和文字；Windows 覆盖原生通知 XML 与快捷方式标识。

发布前用实际 Linux x64 安装包验证项目凭据能直接进入手机号页面并保留已有配置。下载正式资产后核验五个平台安装包、SHA256SUMS、GitHub 资产摘要、可执行文件、TDLib 和许可证；实际 macOS ARM 安装包也通过相同登录启动验证。测试没有提交真实手机号或验证码。

Homebrew 配方由发布任务同步（`58db450`）；自有 Scoop [同步任务 37350532043](https://github.com/YoisakiKnd/scoop-bucket/actions/runs/37350532043) 成功（清单提交 `08d80fd`）。两份包清单与 Release 附带清单逐字节一致，版本、下载地址、哈希及 MIT 元数据均已核验。Windows ZIP SHA-256：`d93a8735a991bb1f34430da743b2b24aaf595082ff554b9e1a8abeb0d23b36d4`。没有为 eimer 创建或更新 PR。

Scoop 仓库已更名为 `YoisakiKnd/scoop-bucket`，旧 `scoop-teleaf` 地址重定向到同一仓库；新安装命令使用新地址。Windows 实际通知弹窗、声音、勿扰模式和终端 GPU 图片显示仍需桌面实机验证，CI 与 ConPTY 不能代替这些检查。可先运行 `teleaf --test-notification` 做无需登录的静音通知测试。

## 0.1.4 发布验证

2026-10-06 已发布 [v0.1.4](https://github.com/YoisakiKnd/teleaf/releases/tag/v0.1.4)，最终标签对应 `b91ecacb195336aa0b3a4aa7bb8024d35e9f660c`。本版补齐 macOS / Linux 桌面通知，统一通知开关和测试命令，并重写 README 首页，将使用、配置和开发说明拆分到 `docs/`，完整文档随包分发。

[分支 CI 37398631178](https://github.com/YoisakiKnd/teleaf/actions/runs/37398631178)、[标签 CI 37398634129](https://github.com/YoisakiKnd/teleaf/actions/runs/37398634129) 及 [正式发布 37398633984](https://github.com/YoisakiKnd/teleaf/actions/runs/37398633984) 均通过。五个平台各 127 项单元测试，以及 Clippy、打包检查和 TDLib 加载通过；Linux 使用私有 D-Bus 验证替换、撤回、线程重连和服务重启后的编号重置，macOS 验证随包助手内容，Windows 覆盖 XML/快捷方式身份及 ConPTY 输入。

首次构建在共享 Intel CI 上触发已有图片测试的 2 秒等待超时，未生成 Release。将测试总等待上限改为 20 秒、保留全部显示断言后，更新尚未发布的标签并重跑五个平台；最终安装包仅来自全部通过的构建。

发布前用实际 Linux x64 安装包验证内置项目凭据能直接进入手机号页且保留已有配置。正式 macOS ARM 包在本机通过相同启动检查，以及通知助手签名、内容自检与系统服务连接检查。源码助手的隔离测试命名空间撤回接口也通过。以上检查没有提交手机号或验证码，也没有弹通知或申请通知权限。

正式下载的五份压缩包、SHA256SUMS、GitHub 资产摘要、TDLib、许可证和新版文档均已核验；README 与三个指南的文字内容与仓库一致（Windows 使用 CRLF 行尾）。Homebrew 配方由发布任务同步（`dde0688`）；自有 Scoop [同步任务 37399219572](https://github.com/YoisakiKnd/scoop-bucket/actions/runs/37399219572) 成功（清单提交 `de23701`）。两份安装清单与 Release 附带文件逐字节相同，版本、URL、MIT 元数据和哈希一致。Windows ZIP SHA-256：`be4dde6cd2f1be1dcb902018da10b09aee4a13b997b9a768778d96b1cff37b2a`。没有为 eimer 创建或更新 PR。

实际横幅、声音、通知权限拒绝后的桌面行为及专注/勿扰模式仍需对应平台实测；可运行 `teleaf --test-notification` 检查静音提醒，macOS 首次显示时需允许系统通知权限。详细配置见 [通知指南](docs/CONFIGURATION.md#桌面消息通知)。本轮没有重新测量真实账号内存；通知线程、队列和后台组件的生命周期限制见配置指南。

## 0.1.5 发布验证

2026-10-06 已发布 [v0.1.5](https://github.com/YoisakiKnd/teleaf/releases/tag/v0.1.5)，标签对应 `e46814fd80e464e3493a80604f0e7ae5b0fb4459`。本版补齐原生 Windows 的 CF_DIB 截图粘贴及错误提示；位图回退在解码前检查大小，RGB 截图直接编码 PNG，减少整图拷贝。README 和使用指南说明终端文字粘贴与 F7 图片粘贴的区别。

[分支 CI 37404445767](https://github.com/YoisakiKnd/teleaf/actions/runs/37404445767)、[标签 CI 37404705255](https://github.com/YoisakiKnd/teleaf/actions/runs/37404705255) 和 [发布任务 37404705217](https://github.com/YoisakiKnd/teleaf/actions/runs/37404705217) 全部通过。五个平台各 131 项 Rust 测试、7 项打包测试、Clippy 和随包 TDLib 加载检查成功，实际 Linux x64 发布包通过内置凭据登录启动门禁；未提交手机号或验证码。

Windows CI 在一次性桌面写入合成剪贴板，ConPTY 验证 CF_DIB 截图、损坏 PNG 的位图回退、F7 / 直接传给程序的 Ctrl+V、关闭鼠标后粘贴、取消暂存清理、损坏和超大位图，以及多行文字。首次端到端测试在演示会话尚未加载时按 Enter；改为等待会话和消息出现后重跑成功，保留完整粘贴断言。普通本地运行跳过剪贴板写入测试。真实终端的默认粘贴键绑定及用户截图工具仍可按使用指南自行验收。

已下载五个平台的正式归档，核验 SHA256SUMS、GitHub 资产摘要、程序、TDLib、许可证和更新后的文档。Homebrew 配方已同步（`84600af`）；自有 Scoop [同步任务 37405233893](https://github.com/YoisakiKnd/scoop-bucket/actions/runs/37405233893) 成功（清单提交 `e6c652c`）。两份安装清单与 Release 附带文件逐字节相同。Windows ZIP SHA-256：`edb15ac378e90e7e62f1085f553392af93e34de51275db5f5c23e3cf738ffcbb`。

## 用户安装

[v0.1.5](https://github.com/YoisakiKnd/teleaf/releases/tag/v0.1.5)及安装清单已发布：

```sh
brew tap YoisakiKnd/teleaf https://github.com/YoisakiKnd/teleaf
brew install YoisakiKnd/teleaf/teleaf
teleaf --check
```

```powershell
scoop bucket add teleaf https://github.com/YoisakiKnd/scoop-bucket
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
 Teleaf Notifications.app/     # 仅 macOS，按需启动的原生通知助手
 LICENSES/
 LICENSE
 README.md
```

- macOS 15+：ARM64、x64。运行库依赖改为 `@loader_path`，附带 Swift 编译的原生通知助手，使用临时代码签名；尚未提供 Developer ID 签名和 Apple 公证。
- Linux：Ubuntu 24.04 的 glibc 基线，x64、ARM64；随附非系统依赖并写入 `$ORIGIN`，glibc 和系统加载器由系统提供。不是 musl/Alpine 通用包。
- Windows 10/11：x64。随附 TDLib 的 DLL 依赖和 MSVC 可再分发运行库；从 DLL 所在目录加载依赖，不要求用户设置 PATH。

TDLib 上游发布包固定为 tdlib-rs `v1.4.0` 中的 TDLib `1.8.61`，平台和 SHA-256 记录在 `packaging/tdlib.json`。运行库许可证随包附带；项目自己的 `LICENSE` 存在时也会附带。更新 TDLib 时需同时更新版本、平台哈希、运行库依赖及加载测试。

## 本地打包验证

在相同系统和架构的构建机运行，不用跨编译的可执行文件代替本机加载检查：

```sh
cargo build --release --locked
python3 scripts/package-release.py --binary target/release/teleaf --version 0.1.5
python3 scripts/test-packaging.py
```

Windows 的 binary 参数为 `target/release/teleaf.exe`，打包机需要 Visual Studio C++ 的 MSVC Redist 目录。Linux 打包机需安装 `patchelf`、`libc++1-18`、`libc++abi1-18` 和 `libssl3t64`。macOS 打包机需要 `openssl@3`、`otool`、`install_name_tool` 和 `codesign`。

`package-release.py` 下载校验过的 TDLib、复制运行库、调整动态加载路径，再从项目目录外运行暂存包的 `teleaf --check`。检查失败则不生成归档。归档不包含 `config.json`、API 凭据、聊天数据库、下载缓存或编译产物目录。

五个平台的产物齐全后，可以独立生成包清单：

```sh
python3 scripts/generate-packages.py --repository YoisakiKnd/teleaf --version 0.1.5 --assets target/dist --output target/packages
```

输出真实配方、Scoop 清单和 SHA256SUMS。该命令拒绝缺失的平台产物和不合法的仓库/版本输入；生成器只写本地文件，不创建远程仓库或发布内容。

## 验证范围

`v0.1.1` 五个平台已通过[原生 CI 验证](https://github.com/YoisakiKnd/teleaf/actions/runs/37119341960)：101 项 Rust 单元测试、7 项打包测试、Clippy、Release 构建，以及暂存发布包的 `teleaf --check` 加载检查。Intel CI 的离线测试在生成 PNG 时触发一秒等待超时，改为有界的总截止时间后五平台通过；全部功能断言保留。Windows 首次发布时因 Python CP1252 无法输出中文失败，此前已显式使用 UTF-8，并加入旧编码环境下的下载和错误提示回归测试。

已重新下载 `v0.1.1` 全部五个平台的实际归档，逐一核对 `SHA256SUMS`、GitHub 资产摘要、主程序、TDLib、项目 MIT LICENSE 和第三方许可；Scoop 清单与 Homebrew 配方的版本、URL 和哈希全部一致。macOS ARM64 发布包在项目目录外独立解压，`--version` 返回 `Teleaf 0.1.1`，`--check` 加载随附 TDLib 1.8.61，未创建账号配置。

本地还验证了 macOS ARM64 归档、依赖重定位、解压后加载及模拟 Homebrew 的符号链接路径；正式发布的 macOS ARM64 包约 11.62 MiB，Homebrew 下载后的 SHA-256 和独立解压加载检查通过。更名后的离线快捷消息 PTY 回归通过，`--help` / `--version` / `--check` 未创建账号数据。

Homebrew 的 tap、配方解析、下载和校验通过，本机实际安装被 Homebrew 的 Command Line Tools 版本检查阻止，尚未完成 `brew test`。遇到同样的提示，请按 Homebrew 提示更新系统开发工具，或直接下载 Release 解压运行。Scoop 实际安装及 Windows 10 实机完整交互尚未验收；Windows CI 已验证发布包包含所需 DLL 并能加载 TDLib。

参照：[Homebrew tap](https://docs.brew.sh/Taps)、[Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)、[Scoop manifest](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests)。
