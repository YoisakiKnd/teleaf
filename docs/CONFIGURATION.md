# 终端与通知配置

[返回 README](../README.md) · [使用指南](USAGE.md)

## 常用配置

| 环境变量 | 用途 |
| --- | --- |
| `TG_THEME` | `auto`、`dark`、`light` 或 `terminal`；后者保留终端背景 |
| `TG_IMAGE_PROTOCOL` | 默认 `auto`；可指定 `kitty`、`sixel`、`iterm2`、`halfblocks` |
| `TG_NOTIFICATIONS` | `0` 关闭；`1` 显式开启，包含 SSH / 无桌面环境 |
| `TG_DATA_DIR` | 自定义账号数据目录；默认路径可在 F4 设置页查看 |
| `TDLIB_PATH` | 自备 TDLib 的动态库路径；正常发行包无需设置 |

macOS/Linux 示例：`TG_THEME=light teleaf`。PowerShell 示例：先运行 `$env:TG_THEME="light"`，再运行 `teleaf`。

这些配置使用已有的 `TG_*` 名称，兼容旧版。非空 `NO_COLOR` 或 `TERM=dumb` 会使用单色界面。

## 多平台与终端适配

使用相同的文件浏览器、鼠标命中和 Unicode 编辑逻辑，无需安装桌面文件选择器。默认探测图片协议，等待最多 250 ms；不响应时回退字符。`TERM=dumb` 和显式指定的协议不发送图片能力查询。探测在启动线程内使用限时读取，超时不遗留 stdin 读取线程，也不会修改已启用的 raw mode。设置页显示当前图片协议、终端、同步输出状态和滚轮行数。

| 环境 | 图片与操作建议 | 适配范围 |
| --- | --- | --- |
| macOS / Linux 的 Ghostty、Kitty | 默认 `auto`；支持 Kitty 图片协议，整帧同步输出 | 协议输出有离线 PTY 检查；[Ghostty 能力说明](https://ghostty.org/docs/about) |
| macOS 的 iTerm2 | 默认 `auto`，探测异常时可强制 `iterm2` | 使用 [iTerm2 图片协议](https://iterm2.com/documentation-images.html) |
| WezTerm | 默认 `auto`，可按终端配置强制 `iterm2` 或 `sixel` | 参见 [官方转义序列说明](https://wezterm.org/escape-sequences.html) |
| Windows Terminal | 正式版 1.22+ 支持 Sixel；默认自动探测，失败时字符回退。保留原生键鼠输入 | [微软正式版说明](https://devblogs.microsoft.com/commandline/windows-terminal-preview-1-23-release/)；原生 ConPTY 输入纳入 CI，GPU 显示仍需实机验证 |
| Linux Wayland / X11 | 默认 `auto`；系统复制使用 `wl-copy` / `xclip` | 上传、文件浏览不依赖这些剪贴板工具 |
| Windows + WSL | 按 Windows Terminal 实际能力显示；盘符路径转换，系统复制使用 `clip.exe` | WSL 运行 Linux 版 TDLib，不加载 Windows DLL |
| SSH / tmux | 能力查询被阻断时用字符模式；复制可通过 OSC 52 写入本机剪贴板 | 原生图片需要外层终端和复用器透传，异常时用回退模式 |
| 其他终端 / 受限控制台 | `halfblocks` 字符预览，`F6` 切换鼠标，`F8` 发送附件 | 图片清晰度取决于实际协议，不承诺所有模拟器的 GPU 显示一致 |

| 环境变量 | 值 / 默认 | 用途 |
| --- | --- | --- |
| `TG_IMAGE_PROTOCOL` | `auto`（默认）、`kitty`、`sixel`、`iterm2`、`halfblocks` | 强制值仅用于确定终端支持该协议时；不会让不支持的终端获得图片能力 |
| `TG_CELL_SIZE` | 如 `12x24`，每个字符单元的实际像素宽×高，各为 1–256 | 探测得到的字体尺寸错误时覆盖；高 DPI 使用实际像素，不是字体字号 |
| `TG_SYNC_OUTPUT` | `1`（默认）/ `0` | 兼容同步输出处理异常的终端或复用器 |
| `TG_SCROLL_LINES` | 默认 `3`，范围 1–12 | 消息和文件浏览器每个滚轮事件的行数；贴纸按网格行移动 |
| `TG_CLIPBOARD` | `auto`（默认）、`system`、`osc52` | `auto` 在 SSH 中用 OSC 52，其他环境用系统剪贴板；需外层终端允许写入 |

OSC 52 仅发送写入请求，不读取剪贴板，也无法确认外层终端是否接受；每次最多发送 100 KB 文字。tmux 中包装为透传序列，仍需复用器和外层终端允许；参见 [Ghostty OSC 52](https://ghostty.org/docs/vt/osc/52)。

Ghostty / POSIX shell：

```sh
TG_IMAGE_PROTOCOL=auto cargo run --release
# 兼容性排查
TG_IMAGE_PROTOCOL=halfblocks TG_SYNC_OUTPUT=0 cargo run --release
```

Windows PowerShell（需要本机 TDLib 及其依赖 DLL）：

```powershell
$env:TDLIB_PATH = 'C:\tdlib\bin\tdjson.dll'
$env:PATH = 'C:\tdlib\bin;' + $env:PATH
$env:TG_IMAGE_PROTOCOL = 'auto'
cargo run --release
# 仅在终端支持 Sixel 时使用：
$env:TG_IMAGE_PROTOCOL = 'sixel'
```

Linux / WSL 使用 `libtdjson.so`，macOS 使用 `libtdjson.dylib`；`TDLIB_PATH` 指向对应系统的库。交互式实测主机为 macOS；五个平台已在原生 CI 构建和测试，Windows ConPTY 输入已验证，原生图片输出有协议测试。Linux/Windows 的交互式终端 GPU 显示及真实账号上传仍需实机验证。PTY 测试使用 Unix API，只能在 macOS/Linux/WSL 运行，不能直接在原生 PowerShell 运行。

## 桌面消息通知

从 v0.1.4 起，macOS、Linux 桌面和 Windows 10/11 支持系统原生提醒，应用运行且登录时接收消息。设置页可用鼠标或 `Tab` / `Enter` 切换 **桌面通知**。开关只影响本次运行；长期关闭可设置 `TG_NOTIFICATIONS=0`（PowerShell：`$env:TG_NOTIFICATIONS="0"`）。SSH 和无桌面的 Linux 默认关闭，可用 `TG_NOTIFICATIONS=1` 显式开启；它通知的是程序所在机器，不转发到 SSH 客户端。

提醒使用 [TDLib Notification API](https://core.telegram.org/tdlib/notification-api)，支持普通消息与提及，遵循 Telegram 通知/静音设置；忽略启动前的历史通知、自己发送的消息和重复事件。同一通知组的新消息替换已有提醒，消息读完或通知撤回后移除对应提醒。只使用文字和媒体类型摘要，不下载通知图片。当前是只读提醒，不支持点击跳转聊天。

| 平台 | 实现和配置 |
| --- | --- |
| macOS 15+ | 安装包附带 **Teleaf Notifications.app**，使用 [UserNotifications](https://developer.apple.com/documentation/usernotifications/unusernotificationcenter)；首次显示提醒时请求系统通知权限。拒绝后可在系统设置 → 通知中开启 Teleaf Notifications，再打开程序里的通知开关。必须保留助手和主程序在同一目录；Homebrew 会自动一起安装。源码构建需要 Xcode Command Line Tools 中的 Swift 编译器。 |
| Linux X11 / Wayland | 直接连接用户会话的 [D-Bus 桌面通知服务](https://specifications.freedesktop.org/notification/latest-single/)，支持通知替换、撤回及静音提示；无需 `notify-send` 或额外 CLI。需要桌面提供 `org.freedesktop.Notifications`（例如 GNOME/KDE 或 dunst/mako）。没有服务时提示并关闭本次提醒，可在启用服务后通过设置重试。已显示编号仅在当前程序内存中保存，可跨空闲线程重启复用；退出重开后不能撤回上一进程遗留的通知。 |
| Windows 10/11 | 原生 WinRT Toast；第一次通知创建当前用户的 **Teleaf Notifications** 开始菜单快捷方式及 `teleaf-notification` 关闭协议，无需管理员权限。点击关闭时处理进程立即退出，不加载 TDLib 或账号数据库。 |

通知线程按需启动、队列最多 16 项、空闲 30 秒后退出；去重和已显示记录有数量上限，不增加聊天轮询。macOS 主程序不加载 AppKit/UserNotifications，仅在处理通知时启动助手，批量合并同组操作后退出；助手运行时会有临时内存占用。Linux D-Bus 连接只在工作线程活动时存在，编号缓存最多 128 组。通知被系统权限、专注/勿扰模式或桌面服务限制时，仍可正常聊天。

先运行 `teleaf --test-notification` 检查系统提醒，无需登录；测试为静音，macOS 首次可能显示授权框。再保持 Teleaf 登录，用另一个账号向未静音会话发消息，检查弹窗与通知中心；读完消息后检查提醒撤回，再检查静音、设置关闭与连续群消息。测试覆盖 TDLib 过滤、Windows XML/快捷方式标识、macOS 助手内容和 Linux 私有 D-Bus 协议。实际横幅、声音和勿扰模式需对应桌面验证。

