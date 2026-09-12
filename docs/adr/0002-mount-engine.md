# ADR-0002 挂载引擎：provider 抽象 + rclone 先行

- 状态：已接受（2026-09-12）
- 背景：需要"把远端存储挂成本地盘符"。可选：rclone 外部引擎、Windows 原生 WebClient、
  自研 WinFsp 文件系统。

## 决定

1. 定义 `MountProvider` trait，应用只依赖抽象；
2. 首个实现为 rclone provider：托管 `rclone rcd`，经 RC API 管 remote 与挂载；
3. Windows 原生 WebClient（`WNetAddConnection2`）作为后续轻量 provider，只覆盖 WebDAV；
4. 自研 WinFsp 文件系统**不做**，除非出现 rclone 与 WebClient 都无法满足的明确需求。

## 理由

- rclone 的 `vfs` 层已经把 Explorer 兼容、缓存、重命名、断线等最深的坑踩完，
  旧版实测可用；自研等于重做一遍这些工作；
- provider 抽象保证引擎可替换，不把应用逻辑焊死在 rclone 上；
- WebClient 路线无驱动、体积极小，但受 50MB 默认文件上限、WebClient 服务依赖、
  认证与重连不可控等限制，适合作为补充而非主路线。

## 后果

- 首期交付需要携带或下载 rclone.exe（分发方式 P4 决策）；
- 挂载依赖 WinFsp（rclone 在 Windows 上的硬依赖）→ 许可见 ADR-0004；
- RC 端口等价 shell 权限，安全要求见 ADR-0003。
