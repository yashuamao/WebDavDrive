# ADR-0001 底座 / 应用边界

- 状态：已接受（2026-09-12）
- 背景：webdav-drive 要重写为 Rust，并作为"可被 Koma 复用的通用底座"的第一个应用。

## 决定

采用"单仓库 workspace = 底座 crates + apps"的结构，边界按**领域无关**判定：

**进底座（foundation-\*）**：错误层级、版本化配置存储、密钥与密钥生命周期、日志环形缓冲、
Job Object、ACL 收紧、计划任务、单实例、Windows 命令行引号、外部进程托管、就绪探测。

**留在应用（apps/drive）**：连接模型、WebDAV/rclone 参数、挂载编排、盘符策略、托盘 UI、
provider 实现。

**绝不进底座**：漫画/媒体领域（series/chapter/archive/scan）、Koma 的 Tauri/Android 细节、
任何 provider 的专有参数。

## 依赖规则

1. `apps → crates → stdlib`，禁止反向依赖；
2. 底座不得依赖 Tauri/Axum/WebView 类型；
3. 平台实现经 trait 注入，Linux 构建不编译 Windows 实现；
4. 底座不得出现 `webdav`/`rclone`/`comic` 等领域词（review 时用 grep 检查）。

## 后果

- Koma 可按 tag 消费单个 crate，不需要跟随本仓库整体演进；
- 抽象不足的部分留在应用里，等第二个真实消费者出现再上移（rule of three）；
- 仓库内禁止"先把接口设计好等未来用"的底座代码。
