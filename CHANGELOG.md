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

### 测试

`cargo test --offline --workspace`：**40 项通过**。

- core 6 · config 7 · secrets 7（含真实 DPAPI 往返）· windows 12（含真实 ACL/Job/单实例）
  · supervisor 8（含 4 项真实子进程测试）

### 尚未实现（后续阶段）

- P2：`apps/drive` 的 Tauri 2 外壳、连接模型、rclone provider、挂载编排；
- P3：托盘 UI（Vue 3）、日志面板、单实例联动；
- P4：自启注册、退出清理、打包（含 rclone 分发与 ADR-0004 许可决策）；
- P5+：Windows 原生 WebClient provider、`foundation-server`、`foundation-tauri`、
  Koma Phase 5 试点接入。

### 注意

- 依赖当前全部命中本机 cargo registry 缓存，构建请加 `--offline`；
- clippy 组件未安装，本版未跑 lint（rustfmt 已跑）。
