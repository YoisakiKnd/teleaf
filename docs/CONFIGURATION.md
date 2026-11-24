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

设置页的 **桌面通知** 开关只影响本次运行。设置 `TG_NOTIFICATIONS=0` 可长期关闭，设为 `1` 可显式开启。焦点上报默认从聚焦开始。终端聚焦且正在查看某个会话时，该会话的新消息静默处理。失焦或切到其他会话后可正常提醒，重新查看时支持撤回的通道会移除该会话的提醒。

提醒数据来自 [TDLib Notification API](https://core.telegram.org/tdlib/notification-api)，支持消息与提及，沿用 Telegram 的静音和预览隐私设置。提醒使用文字和媒体摘要，跳过历史、自己发送和重复的消息。

| 通道 | 自动选择与行为 |
| --- | --- |
| OSC 99 | Kitty。标题、正文和按会话替换，支持撤回与静音。 |
| OSC 777 | Ghostty、WezTerm、Foot 和部分 VTE 终端。支持标题和正文。Ghostty 1.4 以 OSC 777 显示通知。 |
| OSC 9 | iTerm2。标题和正文合为一行。 |
| Windows Toast | Windows 默认使用 WinRT，包括在 WezTerm 等支持 OSC 的终端中运行时。提醒进入 Windows 通知中心。 |
| Linux D-Bus | 本地 Linux 终端无法使用 OSC 时回退到 `org.freedesktop.Notifications` 会话服务。 |
| Terminal BEL | macOS Terminal.app、SSH 远端未识别终端及其他无原生通道的环境。提醒效果取决于终端铃声设置。静音消息跳过 BEL。 |

`TG_NOTIFY_BACKEND` 默认为 `auto`。可设为 `osc`（按环境选择，未识别时用 OSC 99）、`osc99`、`osc777`、`osc9`、`native`、`bell`、`command` 或 `off`。OSC 由终端处理，可通过 SSH 传给客户端。远端未保留外层终端标识时，显式指定对应 OSC；tmux 需允许 `allow-passthrough`，GNU Screen 使用 DCS 透传。OSC 777 和 OSC 9 没有应用侧静音和撤回指令，实际声音由终端及系统设置决定。

可在数据目录的 `config.json` 中添加 `"notify_command": ["/path/to/notify-handler", "--profile", "teleaf"]`，或用 `TG_NOTIFY_CMD` 指定 JSON argv 或单个可执行路径。环境变量优先，空值可覆盖文件配置。配置命令后，`auto` 会选用该命令，也可显式设为 `command`。命令通过 stdin 接收一行 JSON，字段为 `op`（`show` 或 `remove`）、`chat_id`、`title`、`body` 和 `silent`。argv 直接执行，消息内容只进 stdin。每次执行限时 2 秒，输入最多 4 KiB。

运行 `teleaf --test-notification` 可无需登录发送样例并查看所选通道。macOS 的权限归属终端应用，Windows 和 Linux 的原生通知沿用各自通知中心的权限与勿扰设置。

Windows 从 v0.1.6 起会在当前用户注册自己的 `AppUserModelId` 元数据和通知快捷方式；不需要管理员权限。首次发送之前，Windows 可能还没有该应用的通知设置项，查询会返回 `0x80070490`（找不到元素）；此时继续提交第一条实际通知，由系统按权限决定显示，不生成额外的占位通知。遇到旧版本的这个错误时，先确认 `teleaf --version` 并升级，重新运行通知测试。其他错误会注明发生在注册、创建发送器、读取设置或发送阶段；系统关闭通知时也会区分应用、当前用户或组策略。若仍失败，反馈完整错误和 Windows 版本即可。首次查询行为参考 [Microsoft 通知工具包](https://github.com/CommunityToolkit/WindowsCommunityToolkit/blob/main/Microsoft.Toolkit.Uwp.Notifications/Toasts/Compat/ToastNotifierCompat.cs)。
