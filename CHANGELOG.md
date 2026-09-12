# 变更日志

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
- 真机挂载 E2E 就绪：`tests/mount_e2e.rs` 覆盖「本地 WebDAV → 挂成盘符 → 读文件/子目录 →
  卸载后盘符消失」与 AC-38「失败挂载不残留」；缺 WinFsp 时自动 SKIP。

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
