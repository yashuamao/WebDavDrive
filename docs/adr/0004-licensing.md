# ADR-0004 许可与分发

- 状态：待决策（阻塞打包分发，不阻塞开发）
- 背景：挂载链路涉及 WinFsp；引擎 rclone 为 MIT；托盘框架 Tauri 为 MIT/Apache-2.0。

## 事实

| 组件 | 许可 | 影响 |
|---|---|---|
| rclone | MIT | 可随包分发（保留版权声明） |
| WinFsp | GPLv3 + FLOSS 例外 / 商业授权双轨 | 闭源商业分发必须购买商业授权；FLOSS 项目按例外使用 |
| Tauri 2 | MIT / Apache-2.0 | 无约束 |

## 待选项

1. **FLOSS 路线**：项目整体开源，按 WinFsp FLOSS 例外使用；最省事；
2. **商业路线**：购买 WinFsp 商业授权，产品可闭源；
3. **避开 WinFsp**：只用 Windows 原生 WebClient provider，放弃 rclone/多协议；
   代价是 WebDAV-only 与 50MB 默认上限等限制。

## 决定

- 未定之前：可以开发、测试，**不得对外分发二进制**；
- 打包脚本必须把 WinFsp 检测与许可提示做成显式步骤；
- 现状（2026-09-13）：`scripts/package-windows.ps1` 已能产出本地验收目录并提示该限制；
  对外分发（含把 `rclone.exe` 随包发布）仍等待本 ADR 明确选择。
