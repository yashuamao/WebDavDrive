# ADR-0005 Tauri 2 托盘宿主

- 状态：已接受（2026-09-13）
- 背景：webdav-drive 需要一个 Windows 常驻形态：托盘、配置窗口、自启、单实例。
  旧版是"本地 HTTP 服务 + 浏览器配置页"；用户明确选择 Tauri 托盘应用。

## 决定

1. `apps/drive/src-tauri` 是 Tauri 2 宿主，`apps/drive-core` 承载全部业务（无 Tauri 依赖）；
2. 窗口关闭 = 隐藏到托盘；退出只能走托盘菜单/系统退出，退出时停止引擎；
3. 单实例使用 `tauri-plugin-single-instance`，第二个实例聚焦已有窗口后退出；
4. 引擎（rclone）由宿主按需启动，口令与密钥逻辑在 `drive-core`；`drive-pwcmd.exe`
   随程序放在同目录，供 `--password-command` 调用（打包阶段必须一起分发）；
5. 前端首版为无构建 HTML/JS（`apps/drive/ui`），直接使用 Tauri 注入的 `window.__TAURI__`。
   Vue 迁移不阻塞功能，等 P3 需要复用 Koma 组件时再评估。

## 后果

- 业务可被将来其它宿主（CLI/服务）复用，因为 `drive-core` 不知道 Tauri；
- Tauri 依赖较重（首次离线编译约 1 分钟、debug 二进制约 14MB），换取托盘与图标资源；
- `tauri.conf.json` 的 `withGlobalTauri` 打开后，IPC 仅暴露注册过的命令；
  禁止新增任意路径/任意命令类命令（见 ADR-0003）。
