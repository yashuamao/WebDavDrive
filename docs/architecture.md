# foundation 架构

状态：已确认三项基线 —— **provider 抽象 + rclone 先行**、**新仓库 + 旧仓归档**、
**Tauri 2 托盘应用**。本文是实施蓝本。

## 1. 目标与非目标

目标：把"Windows 本地常驻工具"的公共能力沉淀成可复用底座，并用 `apps/drive`
（webdav-drive）作为第一个真实消费者；Koma 后续按需消费同一底座。

非目标：不做漫画领域；不做跨平台挂盘；不自研文件系统；不做多用户账户体系。

## 2. 分层与依赖方向

```text
apps/drive (Tauri 2 宿主)
      │  只做装配：窗口/托盘/命令 → 应用服务
      ▼
apps/drive-core (可选，后续需要 headless 时再拆)
      │
      ├──────────────► foundation-supervisor ──► foundation-core
      ├──────────────► foundation-config    ──► foundation-core
      ├──────────────► foundation-secrets   ──► foundation-core
      ├──────────────► foundation-windows   ──► foundation-core   （cfg(windows)）
      └──────────────► foundation-tauri     ──► foundation-core   （后续，供 Koma 共用）
```

规则：

1. 依赖只能向下；底座 crates **禁止**依赖 Tauri、Axum、WebView 类型；
2. 底座 crates 中不得出现 `webdav` / `rclone` / `comic` / `chapter` 等领域词；
3. 平台能力（DPAPI、Job Object、计划任务、ACL）经 trait 暴露；Linux 构建不编译
   `foundation-windows` 的实现；
4. 应用层拥有领域语义；provider 是可替换适配器，不是底座。

## 3. 底座 crate 职责

| crate | 职责 | 明确不做 |
|---|---|---|
| `foundation-core` | 错误层级与错误码、`Result` 别名、日志环形缓冲（`log` facade）、时钟/ID trait | 不碰文件系统、不碰网络 |
| `foundation-config` | 版本化 JSON 存储：原子写、`.bak`、损坏隔离、版本迁移入口 | 不认识任何具体配置模型 |
| `foundation-secrets` | `SecretStore` trait（DPAPI / 文件回退）、`KeyRing` 密钥生命周期守卫 | 不存业务配置 |
| `foundation-windows` | Job Object、数据目录 ACL、计划任务 XML/注册、单实例锁、Windows 命令行引号 | 不启动业务进程 |
| `foundation-supervisor` | 外部进程托管：spawn、就绪探测（TCP/HTTP）、超时、停止、Job 装配 | 不知道 rclone 是什么 |
| `foundation-tauri`（后续） | 托盘/单实例/窗口显隐/自启开关的 Tauri 装配原语 | 不含任何业务命令 |
| `foundation-server`（后续） | Axum 本地服务：安全中间件、会话/访问码、静态 SPA、health、优雅退出 | 与 Koma Phase 5 共用；Drive 可选 |

## 4. apps/drive 结构（Tauri 2）

```text
apps/drive/
├── package.json / vite.config.ts / tsconfig.json
├── index.html
├── src/                          # Vue 3 前端：连接列表、表单、状态、日志
│   ├── main.ts / App.vue
│   ├── api/                      # invoke 包装 + 视图类型
│   ├── stores/                   # Pinia：connections / engine / ui
│   └── views/                    # Connections / ConnectionForm / Logs / Settings
└── src-tauri/
    ├── Cargo.toml                # 依赖 tauri 2 + 底座 crates
    ├── tauri.conf.json           # 托盘、窗口、单实例、打包
    ├── icons/
    └── src/
        ├── main.rs               # 入口：单实例 → 托盘 → 初始化服务
        ├── lib.rs                # 装配：状态、命令注册、退出清理
        ├── commands/             # profiles / mount / engine / autostart / logs
        ├── service.rs            # AppService：跨命令共享的应用服务
        └── provider/
            ├── mod.rs            # MountProvider trait：probe/mount/unmount/list
            └── rclone.rs         # 首个 provider：rclone rcd + RC API
```

`AppService` 是唯一的应用状态所有者（`Arc`），Tauri `State` 只持有它；
命令层只做参数/错误转换，不写业务逻辑。

## 5. provider 设计

```rust
pub trait MountProvider: Send + Sync {
    fn id(&self) -> &'static str;                       // "rclone"
    fn engine_status(&self) -> EngineStatus;            // 未安装/就绪/错误
    fn probe(&self, conn: &Connection) -> Result<ProbeReport>;
    fn mount(&self, conn: &Connection, target: &MountTarget) -> Result<MountReport>;
    fn unmount(&self, target: &MountTarget) -> Result<()>;
    fn list(&self) -> Result<Vec<MountRecord>>;
    fn shutdown(&self) -> Result<()>;
}
```

rclone provider 细节沿用旧版已验证结论：`rclone rcd` + 随机 RC 口令（环境变量传递）、
`config/create|update|delete` 管 remote、`mount/mount|unmount|listmounts` 管挂载、
配置文件静态加密 + `--password-command` 解密、`vfsOpt`/`mountOpt`/扁平参数映射。
行为规格见 `requirements.md` 第 4 节。

## 6. 运行与生命周期

- 启动：单实例锁 → 加载配置（损坏则隔离）→ 初始化密钥 → 启动 provider 引擎 → 托盘；
- 自启（`--service` 等价）：登录/开机任务拉起后先挂载所有自启连接，再进入托盘；
- 退出：取消进行中的挂载/探测 → 卸载本进程创建的挂载 → 停引擎 → 冲日志；
- 崩溃/强杀：Job Object 回收引擎；下次启动时检测端口/残留挂载并给出修复入口。

## 7. 阶段计划

| 阶段 | 交付 | 验收 |
|---|---|---|
| P0 规格回捞 ✅ | `docs/requirements.md`、ADR、旧文档归档 | 已完成 |
| P1 底座 ✅ | core/config/secrets/windows/supervisor 实现 + 单元测试 | 已完成，测试全绿 |
| P2 挂载 MVP ✅ | `apps/drive-core`（模型/参数/存储/rclone provider/服务）+ Tauri 宿主 | 引擎、配置、安全路径及 WinFsp 真机挂载已验收 |
| P3 托盘 UI ✅ | 托盘/单实例/日志面板/自启注册/无构建 UI 完成；Vue 迁移按需（ADR-0005） | 自动化与 AC-28/38 真机检查通过 |
| P4 交付（部分） | 自启注册/退出清理/打包脚本/release 构建完成；真机挂载验收通过 | AC-28/38 真机通过；FLOSS 许可路线已定，签名、安装器待定 |
| P5 可选 | Windows 原生 WebClient provider；`foundation-server` 管理页 | 按需 |
| P6 Koma 试点 | 用 `foundation-windows`/`foundation-supervisor` 落 Koma Phase 5 一个小切片 | 不阻塞 Koma 主线 |

## 8. 风险

| 风险 | 控制 |
|---|---|
| 底座抽象过度，反向拖慢 drive | 只有 drive 真实需要的才进底座；Koma 需求等 P6 用真实切片验证 |
| Tauri 版本与 Koma 不一致 | 对齐 Tauri 2（Koma 当前 2.x），`foundation-tauri` 延后到有第二个消费者 |
| WinFsp 许可 | 采用 MIT 开源 + WinFsp FLOSS 例外；WinFsp 由用户另行安装，见 ADR-0004 |
| rclone 体积/分发 | Windows 免安装包包含独立 `rclone.exe` 并附 MIT 许可；也支持外部 `RCLONE_EXE` |
| 迁移丢行为 | `requirements.md` 的 AC 清单逐条对照；旧仓库不删 |
