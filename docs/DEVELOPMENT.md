# 开发指南

[返回 README](../README.md) · [发布流程](../RELEASING.md) · [内存预算](../MEMORY_BUDGET.md)

## 环境

需要 Rust 工具链和 Python 3.9+；macOS 还需要 Xcode Command Line Tools 的 Swift 编译器及 `codesign`，用于构建原生通知助手。Linux 通知测试需要 `dbus-daemon`，测试会创建独立总线，不向桌面发送通知。预编译发行包的用户无需安装这些开发工具。

## 从源码运行

需 Rust 工具链和 Python 3.9+。macOS 源码版的预编译 TDLib 仍依赖 Homebrew OpenSSL；发布包则已将其随附。

```sh
# 仅 macOS 源码运行需要这一步
brew install openssl@3
# macOS / Linux；Windows 使用 python scripts/install-tdlib.py
python3 scripts/install-tdlib.py
cargo run --release
```

下载脚本支持 macOS/Linux/Windows 的 x64 和 ARM64，固定 TDLib 1.8.61 并校验 SHA-256，只安装动态运行库，不解压头文件和静态库。Linux 源码版还需系统提供 TDLib 所依赖的 libc++/OpenSSL 等运行库；Ubuntu 24.04 可安装 `libc++1-18 libc++abi1-18 libssl3t64`。已有 `sh scripts/install-tdlib.sh` 命令仍可使用。自备 TDLib 可以设置 `TDLIB_PATH`。

自动发布、Homebrew 配方和 Scoop 清单生成方式见 [RELEASING.md](../RELEASING.md)。

## 源码版登录

未内置项目 API 凭据的源码构建会显示手动设置页。可以在 [Telegram API 页面](https://my.telegram.org/apps) 创建自己的 Desktop 应用，填写 API ID / Hash；已有账号配置继续有效。项目凭据的构建方式和优先级见 [发布指南](../RELEASING.md#项目-api-凭据)。不要提交凭据或账号数据。

macOS 的 `cargo build` 同时在可执行文件旁生成 `Teleaf Notifications.app`；搬动程序时须一起保留该目录。单独 `cargo install` 只复制主程序，无法完成通知助手和 TDLib 的部署；日常安装请使用 Homebrew 或完整发行包。

## TDLib

安装脚本借鉴 [`tgt`](https://github.com/FedericoBruzzone/tgt) 的预编译库分发方式，使用 [`tdlib-rs` v1.4.0](https://github.com/FedericoBruzzone/tdlib-rs/releases/tag/v1.4.0) 的 TDLib 1.8.61，支持 macOS arm64/x86_64，并检查固定 SHA-256。库下载到 `target/tdlib/`；`cargo clean` 后需重新安装。

源码下载脚本使用的 macOS TDLib 依赖 Homebrew 的 `openssl@3`；Teleaf Release 包已随附运行库。若启动提示缺少 `libssl.3.dylib` 或 `libcrypto.3.dylib`，运行 `brew install openssl@3`。自备库可以按 [官方构建说明](https://tdlib.github.io/td/build.html)安装 TDLib，再设置 `TDLIB_PATH`。加载顺序：显式的 `TDLIB_PATH`、可执行文件旁的 `tdlib/`、项目的 `target/tdlib/`、系统库。版本与实际路径在设置页显示。

旧版库（如 TDLib 1.8.0）可能触发 `406 UPDATE_APP_TO_LOGIN`。预编译库能加载不代表服务端一定接受账号登录，需要真实账号验证。

## 本地检查

```sh
cargo fmt --check
cargo test --offline
python3 scripts/test-packaging.py
cargo clippy --offline --all-targets -- -D warnings
cargo build --offline
python3 scripts/test-mouse-pty.py
python3 scripts/test-keyboard-without-mouse-pty.py
python3 scripts/test-login-recovery-pty.py
python3 scripts/test-chat-pty.py
python3 scripts/test-quick-messages-pty.py
python3 scripts/test-kitty-pty.py
python3 scripts/test-attachments-pty.py
python3 scripts/test-terminal-profiles-pty.py
python3 scripts/test-sixel-pty.py
```

测试涵盖配置保存、密钥迁移和权限、登录与粘贴、消息操作、中文/emoji 换行、媒体回退，以及各页面在不同窗口尺寸下的渲染。页面文本预览会写入 `target/ui-previews/`。鼠标回归测试覆盖面板滚动、历史阅读锚点、草稿恢复、Unicode 光标、弹窗隔离及确认按钮防误触，以及跨消息选择、双击替换、拖动滚动条、缩放/平移、图片解码缓存和快速缩放队列。PTY 检查用于 macOS/Linux，会启动独立临时数据目录，不读取现有登录会话；验证真实终端输入解析、鼠标开关、静置和鼠标移动无输出，以及正常退出。`test-keyboard-without-mouse-pty.py` 额外检查关闭鼠标后的登录输入、遮挡显示、聊天输入以及设置页 Tab / Shift+Tab / Enter 操作。

可导出离线夹具的实际字符、颜色和字宽，生成 32 个页面 × 4 个窗口尺寸的 SVG / HTML 预览；此导出代码仅编入测试，不进入客户端：

```sh
NO_COLOR= TERM=xterm-256color COLORTERM=truecolor TG_UI_PREVIEW=1 cargo test all_pages_render_on_wide_and_small_terminals
python3 scripts/render-ui-previews.py target/ui-previews
```

用浏览器打开 `target/ui-previews/index.html`，可切换页面和尺寸。设置 `TG_THEME=light` 可重新生成浅色版本。预览使用合成数据，不代表真实终端的字体、图片协议或 GPU 显示效果。

恢复登录 PTY 测试使用 TDLib 创建真正加密的临时数据库，检查正确密钥、错误密钥、缺少配置、鼠标确认重新登录、旧数据库和文件不变、旧配置备份及重启后目录保持；需要先安装 TDLib。测试不填写真实手机号或验证码。

可以先在离线演示中检查鼠标和页面，无需 API 凭据、TDLib 或账号配置：

```sh
cargo run -- --demo
```

演示包含工作、朋友、归档分组、图片和贴纸；演示消息不会发送到 Telegram。`test-chat-pty.py` 通过真实终端输入解析检查分组/会话点击、聊天内图片回退、放大预览、回复、操作菜单和发送按钮，以及鼠标报告模式、滚轮和静置无重绘。它使用离线数据，不代表真实账号已经完成服务端验证。

`test-quick-messages-pty.py` 在 110×40 和 80×24 的离线终端中检查 `S` / `D`、消息菜单、收藏夹 ID 复用和未发送草稿保留。单元测试还覆盖右键菜单点击、图片/文件/贴纸复制请求、权限差异、过期结果、取消、空结果及收藏夹发送失败。这些检查不向真实 Telegram 账号发送消息；实际账号的收藏与复读仍需手动验证。

`test-kitty-pty.py` 模拟 Ghostty 的图片能力与字体尺寸响应，检查原生图片、高清版本、画布预算、同步帧边界，以及缩放/平移和返回聊天后的输出静止。它验证输出字节流，不验证 Ghostty 的实际 GPU 画面；清晰度和闪烁仍需在真实终端查看。演示的大图现在提供独立的 1280×640 版本供检查。

`test-attachments-pty.py` 检查附件入口、多路径暂存、说明、F8 成组提交、取消、贴纸点击、贴纸包切换和搜索；`test-terminal-profiles-pty.py` 检查 Kitty/Sixel/iTerm2/字符协议输出、dumb 回退与关闭同步输出。均使用独立离线数据，不会给真实账号发送消息。

`test-clipboard-pty.py` 检查 F7、附件粘贴入口、SSH 回退、取消和静置零输出；不读取系统剪贴板。Rust 测试检查剪贴板 PNG 暂存、取消/移除清理、确认后源文件保留、回复/草稿及过期结果；macOS 另外使用私有剪贴板验证 PNG、TIFF、文件列表和文字，不修改用户剪贴板。Windows 和 Wayland/X11 的真实桌面剪贴板仍需对应平台实测。

真实账号登录、服务端功能兼容性和性能目标仍需在目标终端实测。草稿同步、点击通知跳转聊天、语音、群组管理、动态贴纸播放和通话等仍在 [计划](../PLAN.md) 中。

### Windows 原生回归验证

GitHub Actions 的 `windows-input` 作业运行 Windows 单元测试、Clippy 和 ConPTY 输入测试。开发者可在 Windows 上执行：

```powershell
cargo build --locked
python -m pip install pywinpty==3.0.5 pyte==0.8.2
python -X utf8 scripts/test-windows-terminal.py
```

pywinpty / pyte 只用于测试，不是 Teleaf 的运行依赖。测试使用临时数据和离线演示，覆盖登录输入遮挡、设置键盘焦点、关闭鼠标后的输入、F6、窗口调整与 Ctrl+Q 退出；不填写真实手机号或验证码。ConPTY 不等同于 Windows Terminal 的 GPU 显示验证。Sixel 协议和光标位置另有 Rust 单元测试及 Unix PTY 检查；图片输出后恢复后续文字位置，不增加周期刷新。

GitHub 的一次性 Windows CI 环境另外写入合成剪贴板样本，检查 `CF_DIB` 截图、损坏 PNG 的位图回退、F7 / 传递到程序的 Ctrl+V、关闭鼠标后粘贴、取消暂存清理、损坏或超大位图，以及多行文字。普通本地运行跳过这部分，不修改用户剪贴板。位图颜色、方向、行填充、调色板和掩码另有跨平台 Rust 解码回归。

## 媒体管线与性能验证

解码缓存最多 2 张、总计 8 MiB，使用 RGBA8，图片线程空闲 30 秒后清空。原生图片不再一律压到 1024 像素：能放进预算的源图保留细节；大图先按缩放位置裁取原图区域，再缩小并缓存，避免先缩小整图后放大导致细节丢失。字符回退仍按 768 像素最长边及每格两个实际像素处理，避免按字体大小生成无用的大位图。

图片缩放按行执行 Triangle 抗锯齿，不再分配整张浮点中间图；放大/平移直接借用原图区域，不复制完整裁剪图。连续读取每行像素，RGB/灰度等常见格式只分派一次处理方式，减少每个像素的分支判断。加载新源图前先按预算释放旧解码缓存，减少两份源图同时驻留；终端编码缓存按实际适应后的画布估算。RGBA8 的缩小、放大、透明通道和区域裁剪均与 `image` 库 Triangle 输出做逐像素对照。预览源文件最多 32 MiB、1600 万像素、单边 32768 像素，超过限制可用外部程序打开。源文件解码仍需临时内存，缓存预算不等于整个加载过程的内存上限。

编码缓存以 12 份、12 MiB 估算容量为淘汰目标，优先淘汰离屏图片；当前可见图片和等待新视图时的上一帧保持在缓存中，避免容量不足时反复消失和加载。因此屏幕上的图片工作集可能临时超过该目标，离开视图后收回；Kitty 缓存与待处理图片共使用最多 32 个标识，标识只有在对应帧释放后才能复用。任务和结果队列各最多 2 份。图片区域最大 160 列 × 60 行；原生协议聊天预览每边最多 960 像素，大图每边最多 2048 像素，总画布最多 150 万像素。高 DPI 下区域可能缩小，完整原图仍可用外部程序打开。

后台合并同一图片的待处理视图，快速拖动和缩放只处理最新视图；全屏预览时停止处理被遮住的聊天内图片。渲染采用同步输出协议，每帧完整后提交，避免 Ghostty 提前显示半帧；参见 [Ghostty 官方说明](https://ghostty.org/docs/help/synchronized-output)。文件下载进度和无关文件更新不再触发图片重绘。

图片、贴纸只自动下载当前屏幕可见的预览，自动下载最多同时进行 4 个；下载失败后停止自动重试，点击图片可手动重试。文件下载仍由用户操作触发。常见终端通过 Kitty、Sixel 或 iTerm2 协议显示图片，其他终端回退到字符预览。图片处理在后台进行，预览缓存、消息窗口和队列均有容量上限；界面仅在发生变化时重绘。

聊天顶部显示当前群/会话名称，长标题给操作按钮预留空间；窄窗口把部分操作收进更多菜单。消息显示成员姓名，匿名管理员和频道身份使用对应群/频道名称及署名。仅补查当前消息窗口中的缺失发送者，不加载完整群成员列表；姓名缓存最多 1024 项，搜索结果最多保留 100 条，TDLib 更新队列最多 64 条并通过背压保留更新顺序。

日常使用推荐优化构建（首次编译较慢）：

```sh
cargo run --release
```

可重复测量离线图片场景，或只读查看一个已运行的进程：

```sh
python3 scripts/measure-memory.py --binary target/release/teleaf
# Kitty 协议、高 DPI 字符尺寸、4000×3000 合成大图、反复缩放/平移
python3 scripts/measure-memory.py --binary target/release/teleaf --protocol kitty --cell-size 20x40 --large-image
python3 scripts/measure-memory.py --pid 进程号
```

macOS 输出物理内存 footprint 和启动以来的峰值；Linux 输出 RSS 和峰值 RSS，两者统计口径不同。离线场景使用独立临时目录，不加载账号。已登录实例还包含 TDLib 的同步状态和缓存，不能用离线结果代表账号总占用；缓存预算也不代表进程总内存上限。

上一轮逐行缩放优化（解码 12 MiB、编码 16 MiB 预算）在 2026-10-03 的 macOS ARM64、release 构建、180×44 字符窗口、20×40 字符像素、Kitty 协议输出的离线场景中，用同一张 4000×3000 RGB PNG 对照优化前后：

| 场景 | 优化前峰值 footprint | 逐行缩放后峰值 footprint |
| --- | ---: | ---: |
| 打开大图 | 157.7 MiB | 90.2 MiB |
| 反复缩放/平移后 | 195.5 MiB | 97.8 MiB |
| 返回聊天后 | 195.5 MiB | 98.9 MiB |

该场景总峰值约减少 49%，返回聊天时累计 CPU 时间从 0.86 秒降到 0.75 秒。这些是一次隔离演示对照的数据，不含登录账号的 TDLib 占用，也不含 Ghostty 的图片/GPU 内存；协议输出测试不能代替真实终端的画面检查。测量脚本额外输出进程累计 `cpu_seconds`，可在相同操作下比较处理成本，不代表某一帧的加载耗时。

要强制测试字符预览：

```sh
TG_IMAGE_PROTOCOL=halfblocks cargo run
```


## 项目记录

- [功能路线](../PLAN.md)
- [内存预算与实测记录](../MEMORY_BUDGET.md)
- [历史界面评审](../FRONTEND_REVIEW.md)

运行时性能目标、协议测试和真实桌面的验证范围分别记录，离线演示占用不能代表已登录账号的总内存。
