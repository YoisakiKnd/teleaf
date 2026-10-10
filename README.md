# Teleaf

**在终端里使用 Telegram。**

支持鼠标、聊天文件夹、聊天内图片和贴纸预览，以及 macOS、Linux、Windows 桌面通知。仍在开发中，适合日常文字聊天和文件收发。

[下载](https://github.com/YoisakiKnd/teleaf/releases/latest) · [使用指南](docs/USAGE.md) · [终端配置](docs/CONFIGURATION.md) · [更新记录](CHANGELOG.md)

## 功能

- **聊天**：会话分组、历史记录、搜索、回复、编辑、删除、转发和表情回应。
- **媒体**：聊天内图片和贴纸、可缩放的大图预览、图片/文件发送、贴纸搜索与收藏。
- **便捷操作**：鼠标菜单、拖选复制、剪贴板图片/文件粘贴、消息快速收藏和复读。
- **桌面通知**：遵循 Telegram 通知和静音设置，支持同组替换及已读撤回。
- **按需加载**：只准备可见媒体，限制缓存与队列；界面有变化时才重绘。

## 安装

### macOS / Linux · Homebrew

```sh
brew tap YoisakiKnd/tap
brew install YoisakiKnd/tap/teleaf
teleaf
```

### Windows · Scoop

```powershell
scoop bucket add teleaf https://github.com/YoisakiKnd/scoop-bucket
scoop install teleaf/teleaf
teleaf
```

### 直接下载

在 [Releases](https://github.com/YoisakiKnd/teleaf/releases/latest) 下载对应系统和架构的压缩包，按同页 `SHA256SUMS` 校验后解压，运行 `teleaf` / `teleaf.exe`。

请保留完整解压目录，包括 `tdlib/` 和 macOS 的 `Teleaf Notifications.app`。发行包已附带所需运行库，无需自己编译 TDLib 或安装 Rust、Python。

| 系统 | 预编译包 |
| --- | --- |
| macOS 15+ | Apple Silicon、Intel |
| Linux · Ubuntu 24.04 glibc 基线 | x64、ARM64 |
| Windows 10 / 11 | x64 |

Alpine/musl 和 Windows ARM64 暂无预编译包。从源码运行见 [开发指南](docs/DEVELOPMENT.md#从源码运行)。

## 首次使用

1. 运行 `teleaf`，按提示输入手机号、验证码和两步验证密码。
2. 选择会话，按 `Enter` 打开；点击输入框或按 `i` 写消息。
3. 按 `Enter` 发送，发送后继续留在输入框；`Esc` 返回浏览。

**发行包内置 Teleaf 项目 API 凭据，无需自行注册 Telegram 应用。** 登录状态保存在本机，升级后继续使用。若出现 API ID / Hash 表单，请检查是否使用了旧版安装包或未配置凭据的源码构建。

可先体验离线演示，演示消息不会发送到 Telegram：

```sh
teleaf --demo
```

## 常用操作

鼠标可直接选择会话、滚动记录、点击图片和操作按钮；右键消息打开菜单。

| 操作 | 快捷键 |
| --- | --- |
| 选择会话 / 消息，打开 | `↑↓` 或 `j/k`，`Enter` |
| 切换会话与消息面板 / 聊天分组 | `Tab` / `[`、`]` |
| 写消息 / 发送 / 换行 | `i` / `Enter` / `Alt+Enter` |
| 回复 / 编辑 / 搜索 | `r` / `e` / `/` |
| 图片 / 文件 / 贴纸 | `p` / `a` / `t`（浏览时） |
| 附件浏览器 / 粘贴剪贴板 | `Ctrl+O` / `F7` |
| 快速收藏 / 复读选中消息 | `S` / `D`（大写） |
| 取消当前操作 / 回到最新消息 | `Esc` / `End` |
| 帮助 / 设置 / 鼠标开关 | `F1` / `F4` / `F6` |
| 退出 | `Ctrl+Q`，或浏览时按 `q` |

输入时，字母快捷键会正常输入文字。完整快捷键、文字选择和消息操作见 [使用指南](docs/USAGE.md)。

### 发送图片和文件

打开附件浏览器，或直接把文件拖入终端、粘贴本地路径。复制了截图或文件后，用 **F7** 读取剪贴板并进入附件确认页。

确认页可多选文件、添加说明，按 **F5** 切换图片 / 原文件，按 **F8** 发送，`Esc` 取消。Ghostty 的 `Cmd+V`、Windows Terminal 的 `Ctrl+V` / `Ctrl+Shift+V` 是终端文字粘贴；截图使用 **F7**。Windows 截图兼容 PNG、CF_DIBV5 和 CF_DIB 位图。SSH 和 WSL 的剪贴板限制见 [配置说明](docs/CONFIGURATION.md#多平台与终端适配)。

### 图片显示

默认自动检测终端能力。Ghostty / Kitty 可使用 Kitty 图片协议；iTerm2、支持 Sixel 的 Windows Terminal 等使用对应协议。终端不支持时自动回退字符预览，仍可用外部程序打开原图。

图片显示异常时，可在设置页查看当前协议，或尝试字符模式：

```sh
TG_IMAGE_PROTOCOL=halfblocks teleaf
```

PowerShell：先运行 `$env:TG_IMAGE_PROTOCOL="halfblocks"`，再运行 `teleaf`。更多终端与主题配置见 [配置说明](docs/CONFIGURATION.md)。

### 桌面通知

macOS 首次显示提醒时需要允许 **Teleaf Notifications** 的系统通知权限；Linux 需要本地桌面通知服务；Windows 使用原生通知中心。可在 F4 设置页切换 **桌面通知**。

```sh
teleaf --test-notification
```

此命令无需登录，发送一条静音测试通知。SSH 和无桌面的 Linux 默认关闭提醒，通知也不会转发到 SSH 客户端。权限、勿扰模式及关闭方法见 [通知配置](docs/CONFIGURATION.md#桌面消息通知)。

## 升级与排查

- Homebrew：`brew update`，然后 `brew upgrade teleaf`。
- 旧 `YoisakiKnd/teleaf` tap 用户先执行 `brew update` 应用迁移规则；新的分发仓库为 [homebrew-tap](https://github.com/YoisakiKnd/homebrew-tap)。
- Scoop：`scoop update`，然后 `scoop update teleaf`。
- 检查版本和运行库：`teleaf --version`、`teleaf --check`。
- 查看数据目录、图片协议和连接状态：按 `F4` 打开设置。
- 反馈问题：[GitHub Issues](https://github.com/YoisakiKnd/teleaf/issues)。请附系统、终端、版本和复现步骤，并遮挡账号与消息内容。

账号数据沿用 `tg-tui` 目录，升级不会替换登录数据。旧版数据库密钥恢复、重新登录和旧 Scoop 源迁移见 [使用指南](docs/USAGE.md#登录与账号数据)。

## 当前范围

动态贴纸显示静态缩略图；草稿仅保留在本次运行中；聊天文件夹可查看和切换，暂不能编辑规则。语音播放、通话和点击通知跳转聊天尚未实现。

内存占用受 TDLib 同步状态、账号规模和图片解码影响，缓存限制不等于进程总内存上限。工程预算与实测记录见 [内存预算](MEMORY_BUDGET.md)，后续功能见 [路线图](PLAN.md)。

## 开发与许可

[开发与测试](docs/DEVELOPMENT.md) · [发布与包管理](RELEASING.md)

Teleaf 使用 [MIT 许可证](LICENSE)。随包分发的第三方运行库许可证位于 `LICENSES/`。
