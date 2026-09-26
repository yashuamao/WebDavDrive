# 变更日志

## 0.1.9 — 2026-09-26

- 新增应用自更新（设置页「应用更新」面板）：检查新版本 → 下载 NSIS 安装包 → SHA256 校验 →
  卸载全部挂载并停引擎 → 静默安装到当前安装目录 → 自动重启。安装由独立的更新器进程
  `drive-updater.exe` 执行（正在运行的 `drive.exe` 要被替换，不能在自身进程里安装），并等主程序
  退出后才动手；120 秒内主程序未退出就放弃本次更新，不会带着文件锁安装；
- 绿色版（解压即用、目录里没有 `uninstall.exe`）不做自动更新，面板只提示到 GitHub Releases 手动下载；
- 安装包默认目录本地检测：应用每次启动把自身安装目录写入 `HKCU\Software\foundation\WebDAV Drive`，
  安装包新增 NSIS hooks（`apps/drive/src-tauri/nsis/hooks.nsh`），在目录页显示前校验并探测常见安装位置
  （注册表记忆值、`%LOCALAPPDATA%\WebDAV Drive`、`%LOCALAPPDATA%\Programs\WebDAV Drive`、
  `%ProgramFiles%\WebDAV Drive` 等），命中已有安装就把默认目录指过去，重装不必再手选路径；
  静默安装（`/S`）仍以 `/D=` 为准，不做改动；
- 更新源复用引擎更新的镜像前缀与每日限流策略，状态持久化到 `%PROGRAMDATA%\WebDavDrive\app-update.json`，
  更新器诊断日志写到 `app-update.log`；应用启动时会清理遗留的临时更新目录。

验证状态：

- `cargo test --offline --workspace -- --test-threads=1`：全部测试块 0 失败（drive-core 单测 57）；
- 前端 production build 通过；NSIS 安装包实跑静默安装到带空格目录、被动模式下的默认目录探测；
- 自更新链路实跑：更新器进程等待主程序退出 → 静默安装 → 重启新版本。
## 0.1.8 — 2026-09-26

- 修复 NSIS 安装包把引擎装错位置的问题（0.1.6 / 0.1.7 受影响）：`bundle.resources` 由数组改为
  映射形式，`rclone.exe`、`drive-pwcmd.exe`、`LICENSE`、`THIRD_PARTY_NOTICES.md` 现在直接装到
  `drive.exe` 同目录，而不是 `<安装目录>\resources\`；此前装完会提示"没有 rclone 核心"，
  且 `--password-command` 指向的 `drive-pwcmd.exe` 路径也是错的；
- 程序侧兜底引擎发现路径：`<程序目录>\rclone.exe` → `<程序目录>\bin\rclone.exe` →
  `<程序目录>\resources\rclone.exe`（新增，兼容旧安装布局与旧绿色包），并抽出可测的
  `engine_candidates_in(dir)`；
- `--password-command` 解析 `drive-pwcmd.exe` 时同样同时尝试同目录与 `resources\` 子目录；
- 修复"本地没有 rclone 就无法下载引擎"：`update_available` 在缺少本地版本时恒为 false，
  设置页只在它上面判断，导致引擎缺失时没有任何安装入口；现在引擎缺失（`engine.installed == false`）
  时也显示「安装」按钮，并在没有可用版本时提示先点「检查更新」；
- 新增单测：引擎候选路径顺序、以及只在 `resources\` 里存在引擎时仍能被发现。

验证状态：

- `cargo test --offline --workspace -- --test-threads=1`：全部测试块 0 失败（drive-core 单测 29 → 31）；
- 前端 production build 通过；NSIS 安装包静默安装到临时目录后核对实际落盘布局。

## 0.1.7 — 2026-09-26

- 新增 rclone 引擎独立更新（设置页「引擎」面板）：可在不发新版的前提下单独升级 rclone.exe；
- 更新源走 rclone 官方 GitHub release（api.github.com/repos/rclone/rclone/releases/latest），
  下载 rclone-v<ver>-windows-amd64.zip 并用同一 release 的 SHA256SUMS 校验，网络层用系统 WinHTTP
  （不引入 TLS 依赖，保持 --offline 构建）；支持填写镜像前缀与「从本地文件安装引擎」；
- 检查策略：手动按钮随时可用（绕过限流），自动检查每天最多一次，状态与 ETag 持久化到
  %PROGRAMDATA%\WebDavDrive\engine-update.json；
- 安装采用无挂载门禁：卸载全部挂载并确认列表清空 → 停引擎 → 替换 rclone.exe（旧文件留 .old）
  → 重启后校验 core/version 版本号与 options/get 里的 vfs.DirCacheTime → 失败自动回滚；
- 相关单测（择版 / SHA256SUMS 解析 / 每日限流 / 版本比较 / 替换与回滚）；
- 真实联网验证：官方 latest=v1.75.1、zip 31,488,864 B 与 SHA256SUMS 比对一致、解包 rclone.exe
  85,192,704 B 且版本输出 rclone v1.75.1。

验证状态：

- cargo test --offline --workspace -- --test-threads=1：25 个测试块 0 失败（drive-core 单测 10 → 29）；
- 前端 production build 通过。

## 0.1.6 — 2026-09-26

- 分发形态改为 **NSIS 安装包**（`bundle.targets = ["nsis"]`、`installMode: currentUser`）：安装向导自带
  「选择安装目录」页，默认 `%LOCALAPPDATA%`，用户可改到其它盘；再次安装与静默更新会沿用上次选择的目录
  （Tauri 模板的 `RestorePreviousInstallLocation`），无需自定义模板即可满足"不想装在 C 盘"；
- 修复安装包内容不完整：现在随包带上 `rclone.exe`、`drive-pwcmd.exe`、`LICENSE`、
  `THIRD_PARTY_NOTICES.md`（此前 NSIS 包里只有 `drive.exe` 与前端，装完无法工作）；
- 新增 `scripts/build-installer.ps1`（构建 NSIS 安装包 + 输出 SHA-256 + 自动暂存资源）与
  `scripts/sync-version.mjs`（版本号单一来源，`--check` 供发版前校验）；
- 前端本地加入 `@tauri-apps/cli`（devDependency）；
- 修复连接编辑弹窗三个开关的圆点位置：`.toggle-field > span` 误命中 base-ui 渲染的
  `<span role="switch">` 轨道，把轨道变成纵向 flex（圆点顶到上沿、选中态探出右边界），改为显式类
  `.toggle-field-copy`；
- 绿色 zip 作为旁路产物保留。

验证状态：

- `cargo test --offline --workspace -- --test-threads=1`：80 项通过 / 0 失败；
- 前端 production build 通过；NSIS 安装包与绿色 zip 均已产出，SHA-256 见 Release 资产。

## 0.1.5 — 2026-09-25

- 修复挂载设置在「已经挂载的驱动器」上保存后不生效的问题（目录缓存时间、VFS 缓存模式、卷标、
  只读、附加参数、地址/账号等）：rclone 的 RC API 只能创建/销毁挂载，没有修改活动挂载 vfs
  选项的接口，所以设置只在创建挂载那一刻生效。现在保存时会自动按新参数卸载并重新挂载一次，
  改完「目录缓存时间」立刻就能看到效果；
- 重新挂载前先校验新参数：挂载点写错不会把一个还能用的盘卸掉却挂不回来；失败时错误消息会说明
  当前处于「旧挂载还在」还是「已卸载但没挂上」，用户据此手动重挂或直接重试；
- 只改名称、开机自动挂载等与挂载无关的字段，不会打扰正在使用中的驱动器；
- 修复连接编辑弹窗里「网络驱动器 / 只读 / 启动时自动挂载」三个开关的白色圆点没有垂直居中的
  问题（圆点 14px、轨道内框 18px，原来顶对齐，视觉上偏高）；
- 开关选中态的圆点位移由 17px 调整为 18px，左右留白与未选中态对称。

验证状态：

- `cargo test --offline --workspace -- --test-threads=1`：80 项通过 / 0 失败 / 0 忽略，
  rclone 引擎、引擎回收、WinFsp 挂载 E2E 均实际执行；
- 前端 production build（`tsc -b && vite build`）通过；
- `WebDavDrive-0.1.5.zip` SHA-256：
  `ec5d4076470998a5b3962f08aeadbd88c71d748c6e1229a9d9167283f513c0ce`。

## 0.1.4 — 2026-09-15

- 修复 0.1.3 正式包启动后仍访问 Vite 开发服务器，导致页面显示 `localhost` 拒绝连接的问题；
- Windows 打包与重建脚本现在显式启用 Tauri `custom-protocol`，正式包会直接加载内嵌的 React UI；
- 增加正式打包配置回归约束，避免后续再次遗漏 production protocol。

## 0.1.3 — 2026-09-15

- 桌面界面迁移到 React、TypeScript、Vite、Tailwind CSS、shadcn/ui 与 Base UI；
- 驱动器页改为紧凑列表，挂载/卸载作为行内主要操作，其余操作收进更多菜单；
- 统一挂载、忙碌和错误状态语言，连接错误改为行内反馈；
- 增加系统/浅色/深色外观、键盘焦点、Dialog 焦点管理和减少动态效果支持；
- 保留全部 Tauri command、自动挂载、日志、托盘退出清理与 5 秒状态刷新行为。

验证状态：前端 production build 通过；Rust workspace 76 项测试通过；Tauri debug build 通过。

## 0.1.2 — 2026-09-15

- 修复发布版启动时报 `Command plugin:event|listen not allowed by ACL`，导致界面初始化失败的问题；
- 为 Tauri 主窗口补充最小事件监听权限；
- UI 按钮现在先于可选事件订阅完成绑定，即使订阅异常，日志、新建连接等基础功能仍可使用；
- 增加 capability 与前端初始化顺序回归检查。

验证状态：

- `cargo test --offline --workspace -- --test-threads=1`：69 项通过 / 0 失败 / 0 忽略，
  rclone、引擎回收与 WinFsp 挂载 E2E 均实际执行；
- 发布版 WebView 冒烟通过：无 ACL 初始化错误，日志 IPC 返回数组，日志页面可正常打开；
- `WebDavDrive-0.1.2.zip` SHA-256：
  `c65dc3c223cfcca848d67711b6bae797a3d54d801b064d1c3bc90e8fc5b0fc66`。

## 0.1.1 — 2026-09-15

首次公开版本：

- 重建桌面界面，以 RaiDrive 的连接管理体验为视觉参考；
- 支持从连接卡片直接在资源管理器中打开已挂载驱动器；
- 托盘“退出并卸载所有驱动器”会先卸载并确认挂载消失，再结束 rclone 与应用；
- 若退出卸载失败，界面会显示失败项并提供重试或强制退出；
- 项目改为 MIT 开源，补齐 rclone、WinFsp、Tauri 的第三方项目说明与发布包许可文件；
- 修复 CMD 窗口闪现和托盘双图标问题。

### CMD 窗口闪现

现象：GUI 宿主运行期间反复弹出 CMD/控制台窗口，一闪即退。

原因：所有子进程都从 GUI 进程启动，但未标记 `CREATE_NO_WINDOW`，且 `drive-pwcmd.exe`
是控制台子系统程序：

| 子进程 | 触发频率 | 说明 |
|---|---|---|
| `schtasks /Query` ×2 | **每 5 秒**（界面 `autostart_status` 轮询） | 最主要的“一直弹”来源 |
| `schtasks /Create|/Delete` | 注册/移除自启时 | |
| `icacls` | 每次启动收紧 ACL | |
| `rclone.exe`（引擎） | 引擎启动 / 配置加密迁移 | 控制台程序 |
| `drive-pwcmd.exe` | rclone 每次读取配置口令 | 控制台程序，被 rclone 反复调用 |

修复：

- `foundation-core::process::hide_console()`：统一给子进程加 `CREATE_NO_WINDOW`
  （非 Windows 空操作）；`icacls`、`schtasks`（查/建/删）、`rclone` 引擎、
  `rclone config encryption set` 全部接入；
- `drive-pwcmd` 标记 `#![windows_subsystem = "windows"]`：rclone 调用它時不再分配控制台，
  stdout 仍通过管道交给 rclone，不影响 `--password-command`；
- 界面不再每 5 秒查询自启状态（实测每 5 秒 2 次 `schtasks` + 2 个 `conhost`，即每分钟
  24 次进程创建）；改为启动时、手动「刷新」以及注册/移除操作后查询；
- 说明：`target\debug\drive.exe` 是调试构建，本身会保留一个控制台窗口（设计如此）；
  发布/打包产物为 `windows_subsystem = "windows"`，无控制台。

### 托盘双图标（同轮修复）

现象：托盘区出现两个 WebDAV Drive 图标，其中一个右键无菜单、点击无反应。

原因：托盘图标被创建了两次。

- `tauri.conf.json` 的 `app.trayIcon` 会让 Tauri 在 `build()` 阶段（`setup` 钩子之前）
  自动创建一个 id 为 `main` 的托盘图标（tauri 2.11.5 `src/app.rs` 的
  "initialize default tray icon if defined"）；它既没有菜单，也没有本应用注册的
  点击/菜单事件处理器，所以「点了没反应」；
- `setup_tray()` 又用 `TrayIconBuilder::new()` 创建了第二个（有菜单、左键显示主窗口）。

修复：

- 删除 `tauri.conf.json` 的 `app.trayIcon`，托盘图标只由 `setup_tray()` 创建一次；
  `tray-icon` feature 已在 `apps/drive/src-tauri/Cargo.toml` 显式启用，不再依赖该配置；
- 防回归：`setup_tray()` 检测到配置里仍有 `trayIcon` 时记 warning；
  新增单测 `tray_icon_is_not_declared_in_config`（把配置临时加回去验证过会 FAILED）；
- 附带收益：单实例插件在 `build()` 之后才初始化，此前第二个实例会在退出前先建出
  配置托盘图标（托盘闪一下多一个图标）；去掉配置后不再出现。

验证状态（2026-09-15，`--test-threads=1`）：

- `cargo test --offline --workspace -- --test-threads=1`：**67 项通过 / 0 失败 / 0 忽略**；
  真机项（DPAPI、ACL/Job、真实 rclone 集成、引擎回收、WinFsp 挂载 E2E）全部实际执行并通过；
- `cargo check --offline --workspace`、`cargo build --offline --release -p drive -p drive-core`：通过；
- 打包：`package-windows.ps1` 现在**找不到 rclone.exe 直接报错退出**（不再静默产出
  缺引擎的包），并新增 `Z:\AI\webdav-drive\bin\rclone.exe` 作为开发机回退候选（与 drive-core
  集成测试的查找约定一致）；公开版产物为 `dist\WebDavDrive-0.1.1\` 与 `WebDavDrive-0.1.1.zip`，
  **包含 rclone v1.75.1**、项目 MIT 许可和第三方声明（ZIP SHA-256：
  `0E24342D9A56DC6B93A693DF6B3F7B5887E9FFA3BFF7906A048B2B175E305FE9`）；
- 真机验证：启动打包产物，主界面显示「**rclone 已找到**」（bundled 引擎被识别）；
- 注意：`scripts\package-windows.ps1` 必须保存为 UTF-8 **BOM**——本轮编辑丢过一次 BOM，
  Windows PowerShell 5.1 按 ANSI 读中文后直接语法错误（已补回，见 0.1.0 P4 记录的同类坑）；
- 真机托盘 A/B（用 UI Automation 读 Win11 托盘「隐藏的图标」区，同机同环境）：
  - 修复前二进制（临时把 `app.trayIcon` 加回再构建）：该区共 10 个图标，其中
    `WebDAV Drive`（代码创建、有菜单）与一个**无名图标**（配置自动创建、无 tooltip/无菜单）并存；
  - 修复后二进制：该区共 9 个，只剩 `WebDAV Drive` 一个；
- **仍待人工确认**：CMD 窗口闪现是否彻底消失（托盘图标项已真机验证）。
- 环境备注：本会话的 `pwsh` 工具沙箱里，cargo 拉起 build script 会被系统拒绝执行
  （`os error 5 拒绝访问`）；从 bash 调 cargo、或 `powershell.exe -File scripts\package-windows.ps1`
  跑打包脚本均正常（打包脚本已按此方式完整跑通）。

## 0.1.0 — 2026-09-12

新仓库起点：底座边界、规格回收与第一批通用 crates。旧 Python 版（`Z:\AI\webdav-drive`）
判定为误产物，保留原样仅作参考，其行为规格全部回捞进本仓库。

### P0 · 规格与决策

- `docs/requirements.md`：从旧版 README/架构/CHANGELOG + 94/20 项测试回捞行为规格，
  含 38 条验收清单（AC-01..AC-38）、9 组行为规则（挂载点、remote 命名、配置存储、
  密钥生命周期、自启任务、引擎托管、附加参数、本地信任边界、安全红线）；
- `docs/architecture.md`：分层、依赖方向、底座 crate 职责、provider 设计、阶段计划 P0–P6；
- `docs/adr/`：0001 底座边界、0002 挂载引擎（provider + rclone 先行）、
  0003 安全模型、0004 许可（WinFsp 路线待决策，未定前不得分发二进制）；
- `docs/legacy/`：旧版三份文档留档。

### P1 · 底座 crates

- `foundation-core`：统一错误层级 + 稳定错误码、`ChildGuard`/`Clock` trait、
  有界日志环形缓冲（`log` facade；close 后写入不 panic、落盘失败不影响主流程）；
- `foundation-config`：版本化 JSON 存储 `{"version","data"}`；原子写（tmp + fsync + rename）、
  保存前 `.bak`、损坏自动隔离为 `.corrupt-<时间戳>`、未来版本可读但由调用方拒绝写入；
- `foundation-secrets`：`SecretStore` trait；Windows 机器范围 DPAPI（windows-sys）；
  `KeyRing` 守卫"配置已加密但密钥缺失 → 拒绝启动"，损坏密钥显式报错、绝不静默重建；
  非 Windows 提供受限权限文件回退（测试/开发用，文档标注非加密）；
- `foundation-windows`：Windows 命令行引号（MSVCRT 规则）、Job Object（KILL_ON_JOB_CLOSE）、
  数据目录两遍式 ACL 收紧（避免 icacls `/T` + `(OI)(CI)` 把文件 ACL 清空）、
  计划任务 XML + schtasks 注册/查询/删除（PT0S/电池/IgnoreNew、UTF-16 BOM、参数固化引号）、
  命名互斥体单实例；
- `foundation-supervisor`：外部进程托管（凭据走环境变量）、TCP/HTTP 就绪探测（4xx 视为可服务）、
  提前退出与端口占用的区分、宽限后强杀、子进程守卫装配。

### P2 · drive-core 与 Tauri 托盘宿主（2026-09-13）

- `apps/drive-core`：连接模型与校验（AC-01..03、BR-3）、rclone 挂载参数映射（AC-11..18）、
  连接配置存储（AC-04..08，含旧 Python `profiles.json` 自动迁移）、密钥生命周期接入（AC-26）、
  rclone RC 客户端（手写 HTTP/Basic/chunked，含假服务器测试）、`RcloneProvider`
  （静态加密配置 + `--password-command`、RC 口令走环境变量、Job Object、就绪探测）、
  `AppService` 用例编排（保存/探测/挂载/卸载/删除防孤儿/自启挂载）；
- `apps/drive/src-tauri`：Tauri 2 托盘宿主（单实例插件、关闭隐藏、托盘菜单、退出停引擎）、
  11 个 IPC 命令（连接 CRUD、探测、挂载/卸载、状态、日志）、无构建 HTML/JS 界面；
- 真 rclone 集成测试（本机无 WinFsp）：引擎启动 + 配置加密不泄露 remote/密码 +
  未认证 RC 401 + 探测/挂载失败干净返回且引擎存活（AC-23/34/35/36/37）；
- 真机启动冒烟：`drive.exe` 启动后创建 `%PROGRAMDATA%\WebDavDrive`、收紧 ACL、
  选择 DPAPI 后端；第二个实例被单实例插件拦截后退出。

### 测试

`cargo test --offline --workspace`：**70 项通过**（`--test-threads=1`）。

- 底座：core 6 · config 7 · secrets 7（含真实 DPAPI 往返）· windows 12（含真实 ACL/Job/单实例）
  · supervisor 8（含 4 项真实子进程测试）
- 应用：drive-core 单元 6（含自启）+ 参数 6 + 存储 7 + 服务 6 + 真 rclone 集成 2 + 引擎回收 1 + 挂载 E2E 2（无 WinFsp 时 SKIP）
- 另有 `cargo check --offline --workspace` 零警告；`cargo build -p drive` 产出
  `drive.exe` 与 `drive-pwcmd.exe`

### P3 · 自启接入与验收缺口补齐（2026-09-13）

- `drive-core::autostart`：计划任务状态/注册/移除封装；未知模式先拒绝再触系统，
  注册参数为 `--hidden`（计划任务拉起后隐藏到托盘，由应用完成自启挂载）；
- Tauri 新增 `autostart_status` / `install_autostart` / `uninstall_autostart` 命令与界面面板
  （登录时/开机时切换、注册、移除）；
- `AppService` 假 provider 测试 6 项：保存/探测/挂载/卸载/删除全链路、删除防孤儿
  （remote 删不掉或引擎不可达均保留配置）、密码损坏阻断操作、自启挂载只挂勾选项且失败不中断；
- AC-09 用真 rclone 验证更新路径：修改 URL 后再次同步，读回 remote 配置确认已更新，配置仍加密；
- `--hidden` 启动参数：计划任务拉起时窗口不弹出；
- AC-28 真机 E2E：新增 `drive-engine-holder` 测试辅助 bin，启动真实 rclone 后被
  `TerminateProcess` 强杀，验证 Job Object 在 2 秒内回收引擎（`tests/engine_reclaim.rs`）；
- 真机挂载 E2E **已通过**（2026-09-13 安装 WinFsp 2.1.25156 后执行）：
  `mount_success_read_and_unmount` 验证本地 WebDAV → 盘符出现 → 读文件/子目录 → 卸载后盘符消失；
  AC-38 用未知参数触发确定性挂载失败并验证不残留。
  注：不可达 WebDAV 源在 rclone 下是惰性挂载（可能成功返回），不适合作为失败用例；
  另外 provider 现在会自动把 WinFspin 注入 rclone 子进程 PATH，兼容「先启动应用、后安装 WinFsp」的会话。

### P4 · 打包与首启引导（部分完成，2026-09-13）

- `scripts/package-windows.ps1`：`cargo build --release` → `dist\WebDavDrive-<版本>\`，
  含 `drive.exe`、`drive-pwcmd.exe`（必须同目录）、可选 `rclone.exe`、`使用说明.txt`，支持 `-Zip`；
  脚本不注册计划任务、不写注册表，并在输出中提示 ADR-0004 未决前不得对外分发；
- 脚本以 UTF-8 **BOM** 保存：Windows PowerShell 5.1 会按 ANSI 读取无 BOM 的 UTF-8，
  中文直接乱码并导致解析失败（本轮实测踩到）；
- release 构建与打包实测通过（`cargo build --release` 2m30s，产物约 9.6MB + rclone 81MB + zip 31.9MB），
  staged release 启动正常、同目录引擎可被查找；
- 首启引导：界面在找不到 rclone 时显示明确提示（放置位置 / `RCLONE_EXE` / 下载地址）。

### 尚未实现（后续阶段）

- P3：自启注册（foundation-windows task 已就绪，待接入 UI）、Vue 迁移评估、
  AC-10 删除防孤儿的自动化用例、AC-09 update 路径用例；
- P4：打包与引擎分发（含 ADR-0004 许可决策）、签名、首启引导；
- 真机验收缺口：AC-28（强杀宿主回收引擎）、AC-38（挂载失败不残留）、
  无 WinFsp 环境下的挂载成功路径；
- P5+：Windows 原生 WebClient provider、`foundation-server`、Koma Phase 5 试点接入。

### 注意

- 依赖当前全部命中本机 cargo registry 缓存，构建请加 `--offline`；
- clippy 组件未安装，本版未跑 lint（rustfmt 已跑）。
