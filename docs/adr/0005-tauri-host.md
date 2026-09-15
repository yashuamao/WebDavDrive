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
5. 前端首版曾使用无构建 HTML/JS；现已在 `apps/drive/ui` 渐进迁移为 React、TypeScript、
   Vite、Tailwind CSS 与 Base UI 驱动的 shadcn/ui 本地组件。IPC 仍只调用本 ADR 定义的
   Tauri commands，不把业务逻辑移入前端，也不改变 `drive-core` 边界。
6. 自启任务动作固定为 `<自身 exe> --hidden`：计划任务拉起后隐藏到托盘，
   应用启动流程照常执行「挂载所有勾选自启的连接」。
7. 正式构建必须启用 `drive/custom-protocol`。该项目的 Windows 脚本直接调用 Cargo，
   若遗漏此 feature，Tauri 会按开发环境处理 `devUrl`，发布包将错误访问本机 Vite 服务。

## 后果

- 业务可被将来其它宿主（CLI/服务）复用，因为 `drive-core` 不知道 Tauri；
- Tauri 依赖较重（首次离线编译约 1 分钟、debug 二进制约 14MB），换取托盘与图标资源；
- `tauri.conf.json` 的 `withGlobalTauri` 打开后，IPC 仅暴露注册过的命令；
  禁止新增任意路径/任意命令类命令（见 ADR-0003）。
