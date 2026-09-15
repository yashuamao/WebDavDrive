# ADR-0004 许可与分发

- 状态：已接受（2026-09-15）
- 背景：挂载链路涉及 WinFsp；引擎 rclone 为 MIT；桌面框架 Tauri 为 MIT/Apache-2.0。

## 事实

| 组件 | 许可 | 影响 |
|---|---|---|
| rclone | MIT | 可随包分发（保留版权声明） |
| WinFsp | GPLv3 + FLOSS 例外 / 商业授权双轨 | 闭源商业分发必须购买商业授权；FLOSS 项目按例外使用 |
| Tauri 2 | MIT / Apache-2.0 | 无约束 |

## 考虑过的选项

1. **FLOSS 路线**：项目整体开源，按 WinFsp FLOSS 例外使用；最省事；
2. **商业路线**：购买 WinFsp 商业授权，产品可闭源；
3. **避开 WinFsp**：只用 Windows 原生 WebClient provider，放弃 rclone/多协议；
   代价是 WebDAV-only 与 50MB 默认上限等限制。

## 决定

选择 **FLOSS 路线**：WebDAV Drive 以 MIT License 公开源代码并分发 Windows 免安装包。

- WinFsp 作为用户另行安装的系统运行环境，不打入 WebDAV Drive 发布包；
- rclone 可随包分发，发布包必须包含其版权声明与 MIT 许可文本；
- 仓库和发布包必须包含项目 `LICENSE` 与 `THIRD_PARTY_NOTICES.md`；
- 若未来改为闭源商业分发，必须先重新评估 WinFsp 商业授权，不得沿用本决策。
