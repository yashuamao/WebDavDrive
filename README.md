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
  drive/                     # webdav-drive：Tauri 2 托盘 + rclone provider（待建）
docs/
  adr/                       # 架构决策记录
  legacy/                    # 旧 Python 版文档（只读参考）
```

## 构建

```powershell
cargo test --offline          # 当前依赖在本机 registry 缓存中
cargo check --offline
```

> 底座 crates 不得依赖 Tauri、不得出现领域词汇；平台能力经 trait 注入。
> 详细规则见 `docs/adr/0001-foundation-boundaries.md`。
