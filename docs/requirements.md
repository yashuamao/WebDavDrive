# webdav-drive 需求与行为规格（v1）

状态：**从上一版 Python 实现回捞的验收基线**。Python 版是误产物，不保留代码，但它的功能语义、
实测结论和踩坑记录继续有效。新 Rust 实现（`apps/drive`）必须逐条满足本文；有冲突时以本文为准。

来源：旧仓库 `Z:\AI\webdav-drive`（0.1.1）的 README、architecture.md、CHANGELOG.md、
94 项 smoke + 20 项 integration 测试；留档见 `docs/legacy/`。

---

## 1. 产品目标

1. **本地挂盘工具**：把远端存储（首期 WebDAV）挂成 Windows 盘符或目录挂载点。
2. **托盘常驻**：Tauri 2 托盘应用，配置、状态、挂载/卸载都在本地完成，不依赖浏览器。
3. **可扩展到其他挂载源**：挂载引擎经 `MountProvider` 抽象注入，WebDAV 只是第一个 provider。
4. **可无人值守**：注册登录/开机自启，重启后自动恢复已勾选的开机挂载。
5. **底座可复用**：Windows 平台能力、配置/密钥、进程托管沉淀为 `foundation-*` crates，
   Koma 后续（尤其 Windows Web Sharing / NAS 服务）可按 tag 消费，互不阻塞。

## 2. 术语

| 术语 | 含义 |
|---|---|
| 连接 / profile | 一条挂载源配置（名称、地址、账号、盘符、缓存参数、自启开关） |
| 挂载点 | `X:`、`*`（自动分配）或绝对目录（如 `C:\mnt\nas`） |
| provider | 挂载引擎适配器（首期 rclone；后续可加 Windows WebClient 等） |
| 引擎 | provider 背后实际提供文件系统的进程（首期即 rclone rcd） |
| RC | rclone 的 Remote Control HTTP API（回环 + 随机口令） |

## 3. 功能需求

| 编号 | 需求 | 说明 |
|---|---|---|
| FR-1 | 连接管理 | 新建/编辑/删除连接；删除时先卸载再清理引擎侧配置 |
| FR-2 | 凭据保护 | 远端密码不得以可还原形式明文落盘；见 SR-2 |
| FR-3 | 测试连接 | 能在不挂载的前提下探测目标根目录，返回条目数与样例名 |
| FR-4 | 挂载 | 按连接配置挂载；支持盘符、自动分配、目录挂载点 |
| FR-5 | 卸载 | 按连接或挂载点卸载；退出时应清理自己创建的挂载 |
| FR-6 | 状态 | 展示引擎状态、挂载点列表、WinFsp/引擎探测结果、自启状态、日志尾部 |
| FR-7 | 自启 | 注册/移除登录或开机计划任务；启动时自动挂载勾选了自启的连接 |
| FR-8 | 单实例 | 同一数据目录 + 同一服务端口只允许一个实例；重复启动应聚焦已有窗口或明确报错 |
| FR-9 | 日志 | 有界内存环形缓冲 + 落盘文件；UI 可查看；关闭后写入不得抛错 |
| FR-10 | 缓存/VFS 参数 | 每连接可配 VFS 缓存模式、目录缓存时间、卷标、只读、网络驱动器、附加参数 |
| FR-11 | 附加参数 | 支持透传引擎 CLI flag；拒绝进程级危险 flag（黑名单，见 BR-7） |
| FR-12 | 首启引导 | 缺少引擎或 WinFsp 时给出明确指引，不得崩溃 |

## 4. 行为规则（来自旧版实测，必须保持）

### BR-1 挂载点

- 允许 `^[A-Za-z]:$`（统一大写）、`*`（仅非网络模式）、`^[A-Za-z]:\` 开头的绝对目录；
- 目录挂载点不得包含 `..` 片段、`<>:"|?*` 或控制字符，不得是盘符根（`C:\`）；
- `--network-mode` 只接受盘符，与 `*`、目录互斥；
- 挂载点比较不区分大小写。

### BR-2 引擎 remote 命名

- remote 名由连接名 slug + 连接 id 后 8 位组成，只含 `[A-Za-z0-9_]`；
- 连接改名不重建 remote；连接 id/remote 一经生成不变。

### BR-3 配置存储

- 配置为版本化 JSON（`profiles.json`），保存必须原子：先写临时文件、`fs::rename` 覆盖；
- 覆盖前把上一版复制为 `profiles.json.bak`（best-effort）；
- 解析失败**不得静默清空**：把原文件隔离为 `profiles.json.corrupt-<时间戳>` 并记 ERROR，本次以空配置启动；
- 顶层结构非法（不是对象）同样按损坏处理；
- 连接 id 只允许 `[A-Za-z0-9_-]{1,64}`，非法输入直接拒绝（防 URL/HTML 注入）。

### BR-4 密钥生命周期（旧版 bug 修复，必须保持）

- DPAPI 使用**机器范围**（boot/SYSTEM 模式需要无交互解密）；
- 配置加密密钥存 `config_key.enc`（DPAPI 保护）；
- 密钥文件存在但解不开 → **拒绝启动**并提示删除密钥 + 配置；
- 密钥文件不存在但引擎配置已加密 → **拒绝启动**，绝不静默重建（否则旧配置永久无法解开，
  且错误会推迟到第一次配置操作才暴露）；
- 密码解密失败 → 报明确错误，**不得用空值覆盖**引擎中现有凭据。

### BR-5 自启任务

- 计划任务动作必须固化注册时的运行参数：数据目录、端口、引擎绝对路径、Web 目录、
  引擎 RC 端口（或地址），必要时 host/远程开关；
- 参数必须是合法 Windows 命令行：含空格路径逐个加引号（不能用裸 join）；
- 任务 XML 必须显式设置：`ExecutionTimeLimit=PT0S`、`DisallowStartIfOnBatteries=false`、
  `MultipleInstancesPolicy=IgnoreNew`；
- `logon` 模式为默认（当前用户、会话内可见）；`boot` 模式 SYSTEM/全局可见，UI 必须提示
  属主与扩展属性代价；
- 未知模式必须报错，不得退化成 `boot`；
- 状态查询要正确处理本地化控制台编码（旧版曾因 UTF-8 解码崩溃）。

### BR-6 引擎托管

- 引擎进程必须放入 Job Object（`KILL_ON_JOB_CLOSE`）：agent 被强杀/崩溃时内核回收，
  不得留下孤儿进程占着 RC 端口或盘符；
- 启动等待就绪超时（默认 25s）；进程立即退出时要能区分“端口被占用”和普通失败；
- 停止时先 terminate，超时后 kill；
- 退出流程：先卸载挂载，再停引擎，最后关日志。

### BR-7 附加参数

- 语法：`--flag value`、`--flag=value`、裸 `--flag`；解析用 shell 风格引号规则；
- 键名规则：去 `--`、`-` 换 `_`；
- 黑名单拒绝：`password_command`、`config`、`rc_user`、`rc_pass`、`rc_addr`、`rc_no_auth`、
  `rc_serve`、`log_file`（API/UI 输入不得成为命令执行或进程级配置入口）；
- 未识别的 flag 由引擎自行拒绝，错误要原样上报。

### BR-8 本地信任边界（浏览器时代的教训，Tauri IPC 时代同样适用）

- 旧版为由浏览器访问的本地 HTTP 服务，要求：Host 白名单、Origin 同源、POST 仅收
  `application/json`、body 上限 1MB、静态文件路径穿越防护；
- Tauri 版对应要求：IPC command 只接受结构化参数、不开放任意路径/任意命令；
  provider 的路径参数必须 canonical 校验在允许根内；若后续重新提供 Web 管理页，
  上述四条 HTTP 规则必须原样实现（`foundation-server` 负责）。

### BR-9 安全红线

- 内部控制端口只绑回环，凭据每次启动随机生成、不落盘、不进 argv（用环境变量传递）；
- 数据目录必须收紧 ACL：仅 SYSTEM / Administrators / 当前用户；程序启动时执行并记录结果；
- 机器范围 DPAPI 的任何用户可解密 → 文档必须如实说明，不能写“仅管理员”；
- 日志不得输出密码/密钥明文。

## 5. 非目标（本版明确不做）

- 不做漫画/媒体库领域功能（那是 Koma 的事）；
- 不做跨平台挂盘（首期 Windows）；
- 不实现 WebDAV 文件系统语义（缓存/重连/锁交给 rclone provider）；
- 不做多用户账户体系（单机单用户工具）；
- 不做 PWA/公网访问。

## 6. 验收清单（从旧版 94+20 项断言回捞）

### 6.1 配置与校验

- [ ] AC-01 合法 URL（http/https）可保存；非 http(s) 被拒
- [ ] AC-02 名称/地址为空被拒
- [ ] AC-03 非法 id（含引号/尖括号/超长）被拒
- [ ] AC-04 密码保存后不以明文出现于磁盘任意文件
- [ ] AC-05 留空密码编辑不丢原密码；显式清除才置空
- [ ] AC-06 损坏 `password_enc` 解密必须报错，且不得覆盖引擎凭据
- [ ] AC-07 `profiles.json` 损坏被隔离为 `.corrupt-<ts>`，进程以空配置启动
- [ ] AC-08 二次保存生成 `profiles.json.bak`
- [ ] AC-09 连接更新走 engine update，不重建 remote
- [ ] AC-10 删除连接时先卸载再删 remote；引擎不可达时保留连接并报错（防孤儿凭据）

### 6.2 挂载参数

- [ ] AC-11 盘符大写化；`fs` 带冒号
- [ ] AC-12 卷标进挂载选项；VFS 模式/目录缓存进嵌套选项；只读/网络模式为扁平参数
- [ ] AC-13 非法盘符（多字母/空/数字冒号）被拒
- [ ] AC-14 网络模式 + `*` 被拒
- [ ] AC-15 目录挂载点合法透传；`..`/非法字符/相对路径/盘符根被拒
- [ ] AC-16 网络模式 + 目录被拒
- [ ] AC-17 附加参数解析：`--k v`、`--k=v`、裸 flag、带引号值、混合保序
- [ ] AC-18 附加参数黑名单命中即拒

### 6.3 安全

- [ ] AC-19 伪造 Host 被拒（DNS rebinding）
- [ ] AC-20 跨站 Origin 被拒；同源放行
- [ ] AC-21 `text/plain` / 表单类型 POST 被拒
- [ ] AC-22 静态文件路径穿越被拒
- [ ] AC-23 RC 未认证请求被拒（401）
- [ ] AC-24 数据目录 ACL 收紧后，其他本地账户无法读取（人工验收）

### 6.4 密钥与生命周期

- [ ] AC-25 DPAPI 往返一致；同一明文两次密文不同
- [ ] AC-26 加密配置 + 密钥丢失 → 拒绝启动
- [ ] AC-27 密钥损坏 → 拒绝启动并给出删除指引
- [ ] AC-28 引擎进程随 agent 被强杀而回收（Job Object，人工/集成验收）
- [ ] AC-29 日志关闭后写入不抛异常

### 6.5 自启与引擎

- [ ] AC-30 boot 任务 XML 含 BootTrigger / SYSTEM / PT0S / 电池策略 / IgnoreNew
- [ ] AC-31 logon 任务 XML 含 LogonTrigger / LeastPrivilege / 当前账户
- [ ] AC-32 含空格参数被正确引号包裹
- [ ] AC-33 未知自启模式被拒
- [ ] AC-34 引擎 RC 版本可读、只绑回环
- [ ] AC-35 挂载失败返回明确错误且 agent/引擎仍存活
- [ ] AC-36 连接测试对不可达地址返回错误，不伪装成功
- [ ] AC-37 引擎配置静态加密：remote 名与密码不出现在明文
- [ ] AC-38 挂载失败后不残留挂载点记录

## 7. 待定项

- 引擎分发：打包内置 rclone.exe，还是首启引导下载/复用 PATH（影响打包体积与离线安装）；
- WinFsp 许可路线（GPLv3 + FLOSS 例外 / 商业授权 / 仅原生 WebClient provider）；
- 托盘 UI 的最终形态与配置页承载方式（Tauri 窗口 vs 内嵌 Web 页）；
- 是否保留“其他机器浏览器访问管理页”的能力（若保留，`foundation-server` 接手）。

---

## 8. 验收对照（2026-09-13）

自动化测试位置（`cargo test --offline --workspace`，共 68 项）：

| 验收项 | 覆盖位置 | 状态 |
|---|---|---|
| AC-01..03 地址/id 校验 | `apps/drive-core/tests/params.rs` | ✅ |
| AC-04 密码不明文落盘 | `tests/store.rs::dpapi_profile_never_stores_plaintext_password` | ✅（DPAPI 路径） |
| AC-05 留空保留 / 显式清除 | `tests/store.rs` | ✅ |
| AC-06 损坏密码必须报错 | `tests/store.rs::corrupt_password_token_reports_error_instead_of_empty` | ✅ |
| AC-07 损坏配置隔离 | `tests/store.rs::corrupt_profiles_file_is_quarantined_and_starts_empty` | ✅ |
| AC-08 二次保存生成 .bak | `tests/store.rs::second_save_creates_backup` | ✅ |
| AC-09 更新走 update 不重建 remote | `tests/rclone_provider.rs`（改 URL 后再同步，读回配置验证） | ✅ |
| AC-10 删除防孤儿 | `tests/service.rs`（remote 删不掉保留配置；引擎不可达同样保留） | ✅ |
| AC-11..14 挂载点规则 | `tests/params.rs` | ✅ |
| AC-15..16 目录挂载点/网络模式互斥 | `tests/params.rs` | ✅ |
| AC-17..18 附加参数与黑名单 | `tests/params.rs` | ✅ |
| AC-19..22 Web 管理页安全 | 旧 Python smoke 已验证 | ⏸ Tauri IPC 版无 Web 页；启用 `foundation-server` 时移植 |
| AC-23 RC 拒绝未认证 | `tests/rclone_provider.rs::unauthorized_rc_status` | ✅ |
| AC-24 数据目录 ACL | `foundation-windows/src/acl.rs` 测试（属主可读 + 无 Users 组） | ✅ |
| AC-25 DPAPI 往返 | `foundation-secrets/src/dpapi.rs` 测试 | ✅ |
| AC-26 加密配置 + 密钥丢失拒启 | `tests/rclone_provider.rs::encrypted_config_without_key_refuses_to_start` | ✅ |
| AC-27 密钥损坏拒启 | `foundation-secrets/src/keyring.rs` 测试 | ✅ |
| AC-28 引擎随宿主强杀回收 | `tests/engine_reclaim.rs`：真 rclone + TerminateProcess 强杀宿主，验证 Job Object 回收 | ✅ |
| AC-29 日志关闭后不 panic | `foundation-core/src/logging.rs` 测试 | ✅ |
| AC-30..33 计划任务 XML/引号/模式 | `foundation-windows/src/{task,quote}.rs` 测试 + `drive-core/src/autostart.rs`（非法模式先拒、schtasks 查询链路） | ✅ |
| AC-34 RC 只绑回环 + 版本可读 | `tests/rclone_provider.rs` | ✅ |
| AC-35 挂载失败不拖死引擎 | `tests/rclone_provider.rs`（本机无 WinFsp，真实验证） | ✅ |
| AC-36 探测失败不伪装成功 | `tests/rclone_provider.rs` | ✅ |
| AC-37 配置静态加密 | `tests/rclone_provider.rs`（检查密文与明文泄露） | ✅ |
| AC-38 挂载失败不残留挂载点 | 需要 WinFsp 真机 | ⏸ |

已知缺口（按优先级）：AC-38 与「挂载成功」路径需要 WinFsp 真机、AC-19..22 若恢复 Web 管理页再补、
自启注册需要管理员权限（boot 模式），未在自动化中真实注册计划任务。缺口在对应阶段补齐前，不得宣称该阶段完成。
