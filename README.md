# Teleaf

轻量的 Telegram 终端客户端，支持鼠标和聊天内媒体预览，目前仍在开发中。功能路线和性能目标见 [PLAN.md](PLAN.md)。

项目采用 [MIT 许可证](LICENSE)，允许使用、修改、分发及商用，分发时需保留版权和许可声明。随附的第三方运行库遵循各自许可证，发布包中的 `LICENSES/` 提供相关文本。

与 Gemini 3.8 Flash 协作的导航、菜单、贴纸托盘和配色改进已实施，范围和验收记录见 [FRONTEND_REVIEW.md](FRONTEND_REVIEW.md)。

已实现会话列表、历史翻页、文字/图片/文件/贴纸发送、回复、编辑、删除、转发、快速收藏、复读、回应和会话内搜索。图片和贴纸支持终端内预览；不支持图片协议时自动使用字符预览。动态贴纸目前显示缩略图。

低占用目标及各模块预算见 [MEMORY_BUDGET.md](MEMORY_BUDGET.md)：单账号同步结束后静置目标 40–70 MiB，普通聊天 50–90 MiB；这些是需要实测的目标，不是总内存硬上限。

## 开始使用

### 预编译版：Homebrew / Scoop

项目仓库为 [YoisakiKnd/teleaf](https://github.com/YoisakiKnd/teleaf)，运行命令为 `teleaf`。下载 [最新稳定发布包](https://github.com/YoisakiKnd/teleaf/releases/latest)，或按以下方式安装。

**macOS / Linux（Homebrew）：**

```sh
brew tap YoisakiKnd/teleaf https://github.com/YoisakiKnd/teleaf
brew install YoisakiKnd/teleaf/teleaf
teleaf
```

**Windows x64（Scoop）：**

```powershell
scoop bucket add teleaf https://github.com/YoisakiKnd/scoop-teleaf
scoop install teleaf/teleaf
teleaf
```

Scoop 使用独立的轻量仓库 [scoop-teleaf](https://github.com/YoisakiKnd/scoop-teleaf)，添加 bucket 时只克隆安装清单、说明和更新脚本；安装包仍从主仓库的 Release 下载。它每小时检查稳定版并同步清单，GitHub 定时任务可能延迟，也可以手动运行同步。

如果已添加旧的主源码仓库作为 `teleaf` bucket，请执行以下命令切换，无需卸载程序或重新登录：

```powershell
scoop bucket rm teleaf
scoop bucket add teleaf https://github.com/YoisakiKnd/scoop-teleaf
scoop update teleaf
```

升级使用 `brew upgrade teleaf`，或 `scoop update` 后 `scoop update teleaf`。预编译包随附 TDLib、OpenSSL 及所需的非系统运行库，用户无需安装 Rust、Python、CMake，也无需单独编译 TDLib。首批发布目标是 macOS 15+（Apple Silicon / Intel）、Linux glibc（Ubuntu 24.04+，x64 / ARM64）及 Windows 10/11 x64；Alpine/musl 和 Windows ARM64 预编译包暂不在发布矩阵中。

也可以下载 Release 中对应平台的 `teleaf-版本-平台.tar.gz` / `.zip`，根据同页 `SHA256SUMS` 校验后解压；主程序与 `tdlib/` 目录需一起保留，直接运行解压目录中的 `teleaf` / `teleaf.exe`。不要只复制主程序。

安装检查无需打开 TUI 或登录：

```sh
teleaf --version
teleaf --check
teleaf --demo
```

`--check` 检查 TDLib 的加载和版本，不创建客户端或账号数据库。更名继续沿用原 `tg-tui` 用户数据目录及 `TG_*` 配置，不会要求现有用户重新填写 API 凭据或丢弃登录数据；包管理器升级只替换程序目录。

### 从源码运行

需 Rust 工具链和 Python 3.9+。macOS 源码版的预编译 TDLib 仍依赖 Homebrew OpenSSL；发布包则已将其随附。

```sh
# 仅 macOS 源码运行需要这一步
brew install openssl@3
# macOS / Linux；Windows 使用 python scripts/install-tdlib.py
python3 scripts/install-tdlib.py
cargo run --release
```

下载脚本支持 macOS/Linux/Windows 的 x64 和 ARM64，固定 TDLib 1.8.61 并校验 SHA-256，只安装动态运行库，不解压头文件和静态库。Linux 源码版还需系统提供 TDLib 所依赖的 libc++/OpenSSL 等运行库；Ubuntu 24.04 可安装 `libc++1-18 libc++abi1-18 libssl3t64`。已有 `sh scripts/install-tdlib.sh` 命令仍可使用。自备 TDLib 可以设置 `TDLIB_PATH`。

自动发布、Homebrew 配方和 Scoop 清单生成方式见 [RELEASING.md](RELEASING.md)。

首次启动会显示配置表单：

1. 按 `F2` 打开 [Telegram API 页面](https://my.telegram.org/apps)，登录并进入 **API development tools**。
2. 创建应用：应用名和 Short name 自定，Platform 选 **Desktop**。
3. 将官网提供的 **API ID** 和 **API Hash** 粘贴到 TUI，按 `Tab` 切换字段，按 `Enter` 保存。
4. 按页面提示填写手机号、验证码，以及账号启用的两步验证密码。

之后直接运行 `teleaf`（源码版 `cargo run --release`）即可。API 配置会保存在本机，数据库密钥自动生成并保留，已有登录会话由 TDLib 恢复；无需每次设置环境变量。按 `F3` 可以修改 API 配置，保存后自动重新连接。Telegram 要求应用具备自己的 API 凭据，首次领取仍需在其官网完成，参见 [官方说明](https://core.telegram.org/api/obtaining_api_id)。

配置、密钥及账号数据库默认放在系统的用户数据目录，具体路径可在 `s` / `F4` 设置页查看；用 `TG_DATA_DIR` 可以指定其他目录。`config.json` 包含 API 凭据和本地数据库密钥，Unix 上目录权限为 `0700`、文件为 `0600`；配置文件内容本身没有额外加密。请保留该文件，并不要把账号数据提交到仓库。

如果升级前已经用环境变量启动过：原来的 `TG_API_ID`、`TG_API_HASH` 和 `TG_DB_KEY` 会在首次导入时保存。已有数据库缺少密钥时会显示恢复页面，可直接在遮挡的输入框中填写原来的 `TG_DB_KEY`，不需要设置环境变量。它是旧版本使用的本地数据库密钥，不是 API Hash 或 Telegram 两步验证密码。填错会回到恢复页面重试。保存的 API 凭据和本地密钥优先于环境变量；环境变量只在配置缺失时导入，迁移完成后可移除。

不知道旧密钥时，点击“不知道密钥，重新登录”或按 F5，再确认重新登录。API 凭据沿用已填写的内容，新密钥自动生成，新登录使用 `sessions/<随机标识>/` 下的独立目录；旧 `tdlib/`、`files/` 和以前的会话目录保留在原处，旧配置备份在新目录的 `previous-config.json`。重新验证手机号后会同步云端聊天；旧本地数据仍需要原密钥才能读取。以后启动自动使用新的登录目录。

## 界面与操作

界面采用石墨灰背景与薄荷绿强调色：选中会话保留完整行高，未读徽标靠右，长标题和摘要显示省略号；发送、保存与继续按钮突出显示。消息发送者、正文和时间分别呈现，回复/编辑输入区显示目标消息摘要。宽屏与窄屏沿用相同的鼠标操作和键盘焦点规则。

`TG_THEME` 在启动时选择配色，设置页显示当前结果：

| 值 | 效果 |
| --- | --- |
| `auto`（默认） | 根据终端声明选择真彩色或 256 色深色配色；基础终端使用 ANSI 配色 |
| `dark` / `light` | 深色 / 浅色配色；按终端能力降低颜色精度 |
| `terminal` | 保留终端原有背景、透明度及 ANSI 配色 |

例如 macOS/Linux 使用 `TG_THEME=light teleaf` 或 `TG_THEME=terminal cargo run -- --demo`；PowerShell 先执行 `$env:TG_THEME="light"`，再运行 `teleaf`。非空的 `NO_COLOR` 或 `TERM=dumb` 启用单色模式，选中项和主按钮通过反色及加粗区分。配色只在启动时检测一次，未增加动画、定时重绘或媒体缓存；终端自定义 ANSI 色的实际对比度由终端主题决定。

侧栏读取账号已有的 Telegram 聊天文件夹，保持服务器给出的分组名称与顺序；主列表和归档分别显示。点击分组标签切换，点击 `▾` 查看完整分组列表；标签栏可用滚轮和左右箭头浏览，键盘用 `[` / `]` 切换，分组弹窗用方向键和 Enter 选择。每个分组按各自的 `chat.positions` 排序，并通过 `loadChats` 分页加载，不在本地重算包含/排除规则。协议参见 [TDLib 聊天列表说明](https://core.telegram.org/tdlib/getting-started) 和 [文件夹更新](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1update_chat_folders.html)。目前支持查看和切换已有文件夹，新增、编辑文件夹规则仍需使用其他 Telegram 客户端。

宽终端显示会话列表、消息和输入区；窄终端使用单栏，通过 `Tab` 在会话与消息间切换。会话列表显示最近消息和未读数，长消息自动换行。滚轮按行滚动鼠标所在面板，消息选中状态与滚动位置独立；`PgUp` / `PgDn` 也可以滚动消息。最小窗口为 30 列 × 12 行。

| 操作 | 按键 |
| --- | --- |
| 选择会话或消息 | `↑/↓` 或 `j/k`，也可单击；滚轮只滚动内容 |
| 打开会话 | `Enter` 或点击会话 |
| 消息操作菜单 / 媒体预览 | 在消息区按 `Enter` |
| 会话菜单（搜索、历史等） | `Space`，或聊天标题的“更多” |
| 写消息 | `i` 或点击输入区；粘贴文字也会开始输入 |
| 提交 / 换行 / 清空输入 | `Enter` / `Alt+Enter` / `Ctrl+U` |
| 取消当前操作 / 回到会话列表 | `Esc` |
| 帮助 / 设置 | `?` 或 `F1` / `s` 或 `F4` |
| 修改 API 配置 | `F3` |
| 鼠标开关 | `F6`，或设置页点击切换 |
| 复制文字 / 消息 | 有选中文字时 `Ctrl+C`；`c` 或右键菜单也可复制 |
| 快速收藏消息 | 选中消息按 `S`（大写），或右键 → 快速收藏到收藏夹 |
| 复读消息 | 选中消息按 `D`（大写），或右键 → 复读到当前会话 |
| 回到最新消息 | `End`，或点击“回到底部” |
| 退出 | `q`（浏览时）、`Ctrl+Q`（随时），或未选择文字时 `Ctrl+C` |

常用快捷键仍可直接使用：`r` 回复、`e` 编辑、`d` 删除、`f` 转发、`x` 回应、`p` 发图片、`a` 发文件、`t` 发贴纸、`/` 搜索、`g` 历史、`m` 更多会话、`v` 预览、`o` 系统打开。输入时这些字母会正常进入输入框。转发和删除都需要确认；帮助与设置在小窗口内可用方向键滚动。登录字段、继续按钮、消息操作、转发/删除确认、贴纸发送和媒体关闭均可点击。右键消息打开操作菜单，也可点击选中消息旁的“⋯”。点击图片预览区可以打开大图。

输入框支持点击定位光标、左右移动、Home/End 和 Delete；中文、组合字符和 emoji 按完整字符编辑。输入时按 Tab 可转到消息浏览，再点击输入框继续编辑。切换聊天保留本次运行的未发送草稿和光标；发送附件、编辑或搜索时会暂存原草稿，完成或取消后恢复。后台草稿最多保留最近 16 个聊天、合计 512 KiB，超过限制淘汰最久未使用的草稿；退出后不保留，也暂不同步到其他设备。

消息行的 `[回复]` 和 `[⋯]` 只在选中消息上出现，单击文字先选中，右键可直接打开该消息的操作菜单；可重试的失败消息即使未选中也保留 `[重试]`。消息菜单按类型和当前权限显示操作；编辑、为双方删除、转发、收藏及复读读取 [TDLib 消息属性](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1get_message_properties.html)，只保留当前消息的一份权限结果。当前仅支持编辑文字消息。权限更新、目标变化或过期结果不会误操作其他消息。

快速收藏直接转发选中的单条消息到自己的 Telegram **收藏夹（Saved Messages）**，保留原消息来源；首次自动查询收藏夹，之后复用会话 ID。复读直接把选中的单条消息复制到当前聊天，不带转发来源，保留 Telegram 支持的文字格式、媒体和说明。两项都点击即执行，无需先复制文字或选择目标；使用 [TDLib forwardMessages](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1forward_messages.html) 的转发/复制机制，不重新下载或拼接媒体。相册当前只处理选中消息的一张，不会自动复读整个相册。

发送中、发送失败或服务端不允许转发/复制的消息不提供相应快捷操作；只读会话、限流等发送错误会在状态栏提示。操作不改变输入草稿、回复目标或历史阅读锚点；输入框内的 `S` / `D` 正常输入文字。首次收藏查询期间，按 `Esc` 或切换聊天可取消；请求已提交后 `Esc` 不撤回发送。等待服务器确认时显示“已提交…，等待发送”，不会提前提示收藏/复读成功。两项均不自动同意支付 Telegram Stars。这里的消息收藏与贴纸面板的收藏互相独立。

聊天标题的“更多”用于当前会话的搜索、历史及回到最新消息，消息菜单用于该条消息；输入栏保留附件、贴纸和发送三个入口。宽屏顶部显示设置和帮助，窄屏进入聊天后左侧显示返回会话列表。侧栏底部仍可点击加载更多会话。按钮按下有颜色反馈。鼠标启用顺序保证最后开启按钮运动报告，避免部分终端在关闭全量鼠标报告时连点击也一起关闭。Windows 仍使用原生鼠标输入初始化。

消息显示本地时间、日期分隔、发送中、已发送/已读、已编辑及置顶标记。发送失败保存为独立状态，复制或编辑时仍使用原正文；可重试时出现 `[重试]`，也可选中消息按 `R`。重试使用 TDLib 的原失败消息，按服务端是否允许重试执行，不重新拼接正文发送。消息状态字段参见 [TDLib message](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1message.html) 和 [resendMessages](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1resend_messages.html)。

回复消息显示引用摘要（最多 256 字符）；原消息在当前已加载窗口中时，点击摘要可定位原消息，按 `b` 或点击返回按钮恢复引用前的位置。当前仅支持窗口内定位，跨窗口/跨会话的完整引用跳转仍待实现。消息正文沿用原文，窗口按 500 条和约 6 MiB 保留数据估算限制淘汰，搜索结果按 100 条和约 2 MiB 限制；不额外复制一份历史用于引用返回。

阅读历史时，新消息不会改变阅读位置，会显示“新消息 / 回到底部”按钮；已经在底部时自动跟随新消息。滚到聊天历史顶部会自动请求更早消息，同一时刻只发起一次历史请求。侧栏和消息区的滚动条可以点击跳转，也可拖动。按钮需在同一目标按下并释放才执行，拖动、窗口缩放或在目标外释放会取消点击；删除弹窗外的点击不会确认删除。

鼠标移动不会触发重绘，滚轮事件合并处理，连续滚动的重绘频率约为每秒 30 次。图片不会因鼠标悬停下载。若要使用终端自己的选字操作，可按 F6 暂停鼠标，再按 F6 恢复；终端自带的选字修饰键因终端而异。复制消息在 macOS 使用系统剪贴板，Linux 需要 `wl-copy`（Wayland）或 `xclip`（X11），Windows 使用 `clip.exe`；系统剪贴板不可用时界面会提示。

拖选支持跨行、跨消息和反向选择，复制时保留原文换行，自动换行不会产生额外换行。拖到消息区上下边缘会自动滚动；双击文字选择单词或完整表情。选中文字后按 Ctrl+C 或右键复制，Esc 清除选择；输入框中输入或粘贴会替换选中文字，Ctrl+A 全选当前草稿。API Hash 和登录密码保持遮挡，不参与文字选择和复制。

图片和贴纸预览嵌在各自的聊天消息中，随聊天记录滚动；选择其他消息不会替换这些图片。点击消息中的图片即可打开大图，双击媒体标题也可打开。图片滚出边缘时只裁切已编码的预览，不因滚动反复解码或缩小整张图片。滚轮和 +/- 按钮支持相对适应窗口的 1×、1.5×、2×、3×、4× 缩放；放大后拖动图片或用方向键平移，按 0 或点击“复位”回到适应窗口。字符预览也支持同样的缩放和平移，系统打开作为外部查看入口。

聊天内自动加载小预览，点击大图时按需下载 Telegram 提供的最大图片版本；加载和编码期间保留小预览。系统打开使用清晰版本，不再把小缩略图当作原图。

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

## 发送图片和文件

点击输入框下的 **附件**，或按 `Ctrl+O`（输入消息时也可用），打开内置文件浏览器；浏览消息时 `p` 默认使用图片模式，`a` 默认使用原文件模式。

| 操作 | 使用方式 |
| --- | --- |
| 浏览目录 | 点击目录，或 `↑/↓`、`PgUp/PgDn` 后按 `Enter`；`Backspace` / 上一级返回 |
| 多选文件 | 点击文件或按 `Space`，再次选择取消；一次最多 10 个 |
| 粘贴 / 拖入文件 | 支持本地路径、带引号的多个路径、转义空格、每行一个路径和 `file://` URI；支持 `~/` |
| 直接从聊天输入附件 | 粘贴可读取的文件路径自动进入确认页；终端把拖拽当作普通输入时，按 `Enter` 进入确认页 |
| 输入路径 | `Tab` 转到路径栏，输入文件或目录路径后按 `Enter`；浏览器里直接输入字符也会转入路径栏 |
| 查看 / 移除已选文件 | 右侧显示已选列表和图片预览；列表可滚动，点击文件名移除；浏览时 `Delete` 移除最后一个 |
| 切换发送方式 | 点击 **图片模式 / 原文件**；图片模式支持 JPEG、PNG、WebP，原文件模式保留文件字节 |
| 添加说明 | `Tab` 或点击说明栏；`Alt+Enter` 换行；`Ctrl+U` 清空当前字段，最多 1024 个字符 |
| 确认发送 | 点击 **发送附件**、按 `F8`，或终端支持时按 `Ctrl+Enter` |
| 取消 | `Esc` 或取消按钮，保留原消息草稿和光标 |

多文件按选择顺序作为一组发送，说明放在第一项；图片模式组成图片相册，原文件模式组成文件组，保留当前回复目标。分组接口和 2–10 项限制参见 [TDLib sendMessageAlbum](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1send_message_album.html)。混合格式可以统一切换为原文件。发送前检查路径、可读性和图片格式，验证失败保留选择；提交请求后由 TDLib 上传，服务端失败会在聊天消息中标记。

路径只是数据，不执行 shell 命令，也不会读取整个文件再上传；目录列表最多读取 2000 项，大目录可用路径栏直接跳转。只有当前附件预览进入后台图片处理管线，沿用解码和编码预算。发送后保留原来的文字草稿；直接输入路径形成的附件草稿在提交后清除。

拖拽是否发送路径由终端决定。这里支持的是 **文件路径粘贴/拖入**，目前不支持把剪贴板里的截图位图直接粘贴为附件。SSH 会话选择的是远端文件系统中的文件；本机文件须先传到远端。WSL 支持把 `C:\Users\…` 这类盘符路径转换为默认的 `/mnt/c/…`，自定义挂载时直接填写实际 Linux 路径。

## 贴纸面板

点击 **贴纸** 或在消息浏览时按 `t`，在聊天列内、输入框上方打开缩略图面板。宽屏不会覆盖左侧会话列表，输入区仍可点击；点击输入区或按 Esc 关闭面板并继续原来的草稿。点击输入栏的发送按钮提交文字，网格点击 / Enter 提交贴纸。

面板按可用空间调整高度，普通窗口最多 14 行；30×12 等很小的窗口用 emoji 网格保留操作，并暂停无法展示的缩略图下载。

- 顶部是 **最近、收藏、已安装贴纸包** 标签，保持账号贴纸包顺序；点击标签、左右按钮或按 `[` / `]` 切换。
- 中间是按窗口宽度调整列数的图片网格。点击一张直接发送；键盘用方向键选择、`Enter` 发送，滚轮按行浏览，`PgUp/PgDn` 翻页。发送后面板保持打开，方便连续发送。
- 下方支持文字或 emoji 搜索：点击搜索栏、按 `/` 或 `Tab` 输入，`Enter` / 搜索按钮提交。过期搜索结果不会覆盖新查询。
- 选中贴纸后点击 **☆收藏 / ★取消收藏**，或按 `f`；收藏结果从服务器刷新。`Esc` 或点击面板外关闭。

最近、收藏和已安装列表通过 [TDLib](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1get_installed_sticker_sets.html) 读取，搜索使用 [searchStickers](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1search_stickers.html)。贴纸包只在打开时取内容，最多缓存 4 个包，每包 256 项；最近、收藏各最多 256 项，搜索最多 64 项。仅下载当前网格可见的预览，最多 4 个并发；面板遮住的聊天图片暂停准备。动态贴纸仍显示静态缩略图；终端不支持原生图片时使用字符预览或 emoji 占位。

## 多平台与终端适配

使用相同的文件浏览器、鼠标命中和 Unicode 编辑逻辑，无需安装桌面文件选择器。默认探测图片协议，等待最多 250 ms；不响应时回退字符。`TERM=dumb` 和显式指定的协议不发送图片能力查询。探测在启动线程内使用限时读取，超时不遗留 stdin 读取线程，也不会修改已启用的 raw mode。设置页显示当前图片协议、终端、同步输出状态和滚轮行数。

| 环境 | 图片与操作建议 | 适配范围 |
| --- | --- | --- |
| macOS / Linux 的 Ghostty、Kitty | 默认 `auto`；支持 Kitty 图片协议，整帧同步输出 | 协议输出有离线 PTY 检查；[Ghostty 能力说明](https://ghostty.org/docs/about) |
| macOS 的 iTerm2 | 默认 `auto`，探测异常时可强制 `iterm2` | 使用 [iTerm2 图片协议](https://iterm2.com/documentation-images.html) |
| WezTerm | 默认 `auto`，可按终端配置强制 `iterm2` 或 `sixel` | 参见 [官方转义序列说明](https://wezterm.org/escape-sequences.html) |
| Windows 原生终端 | 保留原生鼠标初始化；新版本 Windows Terminal 可用 Sixel，不支持时字符回退 | [Windows Terminal Sixel 说明](https://devblogs.microsoft.com/commandline/windows-terminal-preview-1-22-release/)；Windows 目标编译检查已通过，实机未验证 |
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

Linux / WSL 使用 `libtdjson.so`，macOS 使用 `libtdjson.dylib`；`TDLIB_PATH` 指向对应系统的库。当前实测主机为 macOS；Windows 做了编译检查，其他终端的原生图片输出做了协议测试，Linux/Windows 实机与真实账号上传仍需验证。PTY 测试使用 Unix API，只能在 macOS/Linux/WSL 运行，不能直接在原生 PowerShell 运行。

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
python3 scripts/test-login-recovery-pty.py
python3 scripts/test-chat-pty.py
python3 scripts/test-quick-messages-pty.py
python3 scripts/test-kitty-pty.py
python3 scripts/test-attachments-pty.py
python3 scripts/test-terminal-profiles-pty.py
```

测试涵盖配置保存、密钥迁移和权限、登录与粘贴、消息操作、中文/emoji 换行、媒体回退，以及各页面在不同窗口尺寸下的渲染。页面文本预览会写入 `target/ui-previews/`。鼠标回归测试覆盖面板滚动、历史阅读锚点、草稿恢复、Unicode 光标、弹窗隔离及确认按钮防误触，以及跨消息选择、双击替换、拖动滚动条、缩放/平移、图片解码缓存和快速缩放队列。PTY 检查用于 macOS/Linux，会启动独立临时数据目录，不读取现有登录会话；验证真实终端输入解析、鼠标开关、静置和鼠标移动无输出，以及正常退出。

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

真实账号登录、服务端功能兼容性和性能目标仍需在目标终端实测。草稿同步、通知、语音、群组管理、动态贴纸播放和通话等仍在 [计划](PLAN.md) 中。
