# 变更日志

本文档按版本记录「改了什么、为什么改、怎么验证」。每条都对应一次代码审查的结论，
方便后来人（包括未来的我们）理解当时的取舍，而不是只看到 diff。

---

## 0.1.1 — 2026-09-12

主题：**本地 API 的信任边界**、**凭据不再能被静默破坏**、**自启/进程生命周期的健壮性**。

测试基线：`smoke_test.py` 94 项通过、`integration_test.py` 20 项通过（本机 rclone v1.75.1）。

### 安全（高危）

#### 1. 本地 API 不校验 Host / Origin / Content-Type（DNS rebinding + CSRF）

- **现象**：任意网页都能向 `127.0.0.1:8787` 发恶意请求。实测 `Host: evil.example.com`
  能拿到 `/api/profiles` 数据；带外部 `Origin` 的 POST 能创建连接；`text/plain` 的简单请求
  不触发 CORS 预检，body 又是合法 JSON 时会被服务端照单全收。
- **影响**：DNS rebinding 可让攻击页面同源读写 agent（读连接列表、改配置、挂载攻击者的
  WebDAV、清空密码、增删自启任务）。
- **修复**（`agent/server.py`）：
  - 路由前统一走 `Handler._client_allowed()`：`Host` 必须命中绑定地址/回环别名；带 `Origin`
    的请求必须与 `Host` 同源；
  - `_read_json()` 只接受 `Content-Type: application/json`，并限制 body 大小 ≤ 1 MB；
  - `--host` 绑非回环地址现在必须显式加 `--allow-remote`，否则拒绝启动（返回码 4）。
- **验证**：smoke 新增「伪造 Host 被拒」「跨站 Origin 被拒」「同源 Origin 放行」
  「text/plain 拒绝」「表单类型拒绝」5 项；临时探针实测返回 403/400。

#### 2. profile id 未校验 + 前端未转义 → 存储型 XSS

- **现象**：`/api/profiles` 接受任意 id，`web/app.js` 直接写进 `data-id="${profile.id}"`。
  提交 `x" onmouseover="alert(1)` 可落库并注入事件处理器。
- **修复**：`ProfileStore.upsert()` 只接受 `[A-Za-z0-9_-]{1,64}`，非法即 400；
  `web/app.js` 对 id 以及其他服务端返回字段统一 `escapeHtml()`。
- **验证**：smoke「非法 id 被拒（防存储型 XSS）」「非法 id 被 store 拒绝」。

#### 3. 机器范围 DPAPI 的实际暴露面被文档低估，数据目录默认对所有本地用户可读

- **现象**：README/架构文档写的是「本机管理员可以解密」。但 `CRYPTPROTECT_LOCAL_MACHINE`
  的官方语义是**同机任意用户**都能解密；同时 `%PROGRAMDATA%` 默认给
  `BUILTIN\Users:(OI)(CI)(RX)`，创建的子目录/文件会继承只读权限。两者叠加 = 任何本地账户
  都能读 `profiles.json` + `config_key.enc` 并还原 WebDAV 密码。
- **修复**：
  - `winenv.harden_data_dir()`：启动时用 `icacls` 去掉继承，仅授权 `SYSTEM` /
    `Administrators` / 当前运行账户；日志记录「数据目录权限已收紧」或 ERROR；
  - 文档（README 安全说明、architecture D7）改为正确口径，并说明真正的缓解是外部密钥库。
- **实现坑（已绕过）**：`icacls` 的 `(OI)(CI)` 标记只对目录有效，和 `/T` 混用会让子文件
  变成空 DACL（连属主都读不了）。所以分两遍：先显式授权所有子对象，再给目录设置可继承授权。
- **验证**：临时探针创建「先有文件、后收紧 ACL」的目录，确认原文件可读、新建文件继承
  `(I)(F)` 且不含 `Users`；smoke 的 App 启动路径覆盖该函数。

### 凭据安全（中危）

#### 4. `config_key.enc` 丢失时静默重建密钥

- **现象**：钥匙文件不存在 → 生成新钥匙；`rclone.conf` 仍是旧的加密配置。`rclone rcd`
  启动阶段只验证 `core/version`，所以 agent 显示启动成功，之后每个配置操作才 panic。
- **修复**：`RcloneSupervisor.ensure_config_key()` 在「配置已加密 + 钥匙不存在」时直接抛错，
  提示删除 `rclone.conf` 后重建连接。
- **验证**：smoke「加密配置 + 丢失密钥 -> 拒绝启动」；探针复现修复前/后的行为差异。

#### 5. 密码解密失败被当成空密码，静默覆盖 rclone 里的凭据

- **现象**：`ProfileStore.reveal_password()` 捕获异常返回 `""`；`sync_remote()` 会把空
  `pass` 推给 rclone，破坏原本可用的凭据，用户只看到一次莫名其妙的 401。
- **修复**：`reveal_password()` 解密失败改为抛错；`sync_remote()` 包装成明确的中文错误，
  且**不去改动 rclone 配置**。
- **验证**：smoke「损坏密码解密必须抛错（而不是返回空串覆盖 rclone）」。

### 自启与挂载点（中危）

#### 6. 自启任务丢失运行参数（尤其 `--rclone`）

- **现象**：任务只带 `--data-dir`/`--port`。手工用 `--rclone D:\tools\rclone.exe` 跑通的用户，
  重启后 agent 找不到 rclone，开机挂载静默失败。
- **修复**：注册时把 `--rclone <绝对路径>`、`--data-dir`、`--port`、`--web-dir`、
  `--rc-port`/`--rc-addr`、以及必要时 `--host`/`--allow-remote` 全部写进任务，
  并在日志里打印；找不到 rclone 时直接 400 拒绝注册。`--attach` 模式会记录一条 ERROR
  说明任务将自行拉起 rclone。
- **验证**：smoke 校验 `agent_command()` 输出含 `--service` 且带引号；
  integration/手工注册可通过 `schtasks /Query ... /V` 检查参数。

#### 7. 任务参数不逐个加引号，含空格路径被拆开

- **现象**：`" ".join(extra_args)` 生成的 `--data-dir C:\My Data\WebDavDrive` 会被 Task
  Scheduler 按空格拆成两个参数，pythonw 出错后 stderr 无人接收 → 自启无声失败。
- **修复**：改用 `subprocess.list2cmdline()`。
- **验证**：smoke「含空格的自启参数被引号包裹」。

#### 8. 文档承诺的「挂目录绕开盘符可见性」实现走不通

- **现象**：README/architecture 说可以挂 `C:\mnt\nas`，但 `DRIVE_RE` 只允许 `X:` 或 `*`。
- **修复**：新增 `normalize_mount_point()`，接受 `X:` / `*` / 绝对目录；目录路径拒绝 `..`、
  非法字符和盘符根目录；`--network-mode` 与目录挂载点互斥（rclone 的限制）。UI 的「盘符」
  下拉改为可输入的「盘符或目录」+ datalist，卡片上显示「挂载点」。
- **验证**：smoke 新增目录挂载点正向/反向共 8 项。

### 健壮性（低危）

#### 9. 删除 remote 失败仍删 profile，留下带凭据的孤儿 remote

- **修复**：`do_DELETE` 中 remote 删除失败且 rclone 可达时返回 502 并保留配置；
  只有确认 remote 已不存在才继续删除。
- **验证**：smoke「rclone 不可达时拒绝删除配置（防孤儿 remote）」。

#### 10. `profiles.json` 损坏被静默清空并覆盖

- **修复**：解析失败时把文件隔离为 `profiles.json.corrupt-<时间戳>` 并写 ERROR 日志；
  每次保存前把上一版复制为 `profiles.json.bak`。
- **验证**：smoke「损坏的 profiles.json 被隔离备份」「二次保存生成 profiles.json.bak」。

#### 11. 其他

- `RingLog.write()`：捕获所有异常并加 closed 标志，关闭后写入不再抛 `ValueError`
  （关机竞态下 daemon 线程收尾会踩到）。smoke 有对应用例。
- `RcloneSupervisor`：rclone 子进程放入带 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 的
  Job Object，agent 被任务管理器强杀/崩溃时由内核回收，避免孤儿占着 RC 端口和盘符；
  探针用 `TerminateProcess` 强杀 agent，确认 rclone 随之消失。
- `_wait_ready()`：rclone 启动即退出时探测 RC 端口，把「端口已被占用」和普通启动失败分开报。
- RC 口令不再出现在 argv（Windows 上命令行可能被其他本地用户读到），改走
  `RCLONE_RC_USER` / `RCLONE_RC_PASS` 环境变量；integration 新增「RC 拒绝未认证请求」。
- `--service` 先绑端口再挂载自启项，端口被占时不再白挂一遍。
- `--data-dir` 转为绝对路径；`agent.status.version` 从 `agent.__version__` 读取；
  `autostart.install(mode)` 拒绝未知模式（不再默默退化成 `boot`）；`/api/autostart` 默认
  `logon`；附加参数黑名单拒绝 `password_command` / `log_file` 等进程级 flag。
- 清理未使用的 `urllib` 导入；新增 `.gitignore`。

### 测试

- `tests/smoke_test.py`：69 → **94 项**，新增 API 信任边界、id 校验、密钥丢失、密码损坏、
  配置隔离备份、目录挂载点、引号、日志关闭等用例。
- `tests/integration_test.py`：19 → **20 项**，新增 RC 未认证拒绝；修正「挂载点未残留」
  原先使用挂载前快照导致恒为真的测试缺陷。
- `tests/e2e_test.py`：去掉硬编码的 `Z:\AI\webdav-drive` 安装提示路径。

### 升级注意事项

- 若之前用 `--host` 绑过非回环地址，现在需要显式加 `--allow-remote`。
- 已注册的计划任务建议**重新注册一次**，以带入 `--rclone` 等参数（旧任务仍能用，
  只是参数不全）。
- 删除连接在 rclone 不可达时现在会返回 502 并保留配置，这是有意行为；
  修好 rclone 后重试即可。
- agent 首次启动会对数据目录做 ACL 收紧；日志会记录结果。若在 SYSTEM 创建的数据目录上
  用普通用户运行，收紧可能失败并记 ERROR，此时需要管理员处理一次。
- 老的 `profiles.json` 不需要迁移（格式未变）；挂载点字段开始支持目录路径。

### 已知限制（未在本版处理）

- 同机其他本地用户仍可直接访问回环 API（需要额外认证层才能解决）。
- `boot`（SYSTEM）模式下的属主/扩展属性权限映射仍需要用户自行加 `--fuse-flag`。
- 挂载健康检查、断线自愈仍是 0.3 的路线图内容。
- 本仓库仍没有 LICENSE 文件（依赖许可证分析见 README，但项目自身的授权需要单独决定）。

---

## 0.1.0 — 初始版本

单 agent + rclone RC 编排 + Web 配置界面 + 计划任务自启 + rclone.conf 静态加密。
