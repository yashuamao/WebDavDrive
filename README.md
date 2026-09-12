# foundation

通用代码底座 + 第一个应用 **webdav-drive**（Windows 本地挂盘工具）。

- 底座与应用的边界、决策记录：`docs/adr/`
- 行为规格（从上一版 Python 实现回捞，是新实现的验收基线）：`docs/requirements.md`
- 目标架构与阶段计划：`docs/architecture.md`
- 旧 Python 版文档留档：`docs/legacy/`

## 目录

```text
crates/                      # 通用底座：不出现任何漫画 / WebDAV / rclone 概念
  foundation-core/           #   错误层级、日志、基础 trait
  foundation-config/         #   版本化配置：原子写、.bak、损坏隔离、迁移入口
  foundation-secrets/        #   密钥抽象：Windows DPAPI / 文件回退、密钥生命周期守卫
  foundation-windows/        #   Job Object、ACL、计划任务、单实例、命令行情号
  foundation-supervisor/     #   外部进程托管：就绪探测、回收、停止
apps/
  drive-core/                # webdav-drive 业务核心（无 Tauri 依赖）：模型/参数/存储/provider/服务
  drive/                     # Tauri 2 宿主
    src-tauri/               #   Rust 宿主：托盘、单实例、IPC 命令
    ui/                      #   无构建 HTML/JS 界面
docs/
  adr/                       # 架构决策记录
  legacy/                    # 旧 Python 版文档（只读参考）
```

## 构建

```powershell
cargo test --offline --workspace -- --test-threads=1   # 当前依赖全部命中本机 registry 缓存
cargo check --offline --workspace
cargo build --offline -p drive                          # 产出 drive.exe + drive-pwcmd.exe
```

运行 `target\debug\drive.exe` 需要 `rclone.exe`：放到程序同目录或 `bin/`，或设置 `RCLONE_EXE`。

## 打包（Windows 免安装）

```powershell
.\scripts\package-windows.ps1 -RclonePath <rclone.exe 路径>   # 找不到时自动尝试 bin/ 或 RCLONE_EXE
.\scripts\package-windows.ps1 -SkipRclone -Zip               # 不含引擎 / 额外打 zip
```

产物为 `dist\WebDavDrive-<版本>\`：`drive.exe`、`drive-pwcmd.exe`（必须同目录）、
可选的 `rclone.exe`、`使用说明.txt`。脚本不会注册计划任务、不写注册表。

运行前置：**WinFsp**（rclone 挂载的硬依赖，需用户单独安装）与 `rclone.exe`（MIT，可随包）。
WinFsp 是 GPLv3 + FLOSS 例外 / 商业授权双轨——**ADR-0004 确认前产物不得对外分发**。

> 底座 crates 不得依赖 Tauri、不得出现领域词汇；平台能力经 trait 注入。
> 详细规则见 `docs/adr/0001-foundation-boundaries.md`。
