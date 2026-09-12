# 架构与设计决策

## 1. 进程拓扑

```
┌──────────┐  HTTP    ┌───────────────┐  RC API (127.0.0.1, 一次性口令)  ┌────────────┐
│ 浏览器   │ ───────> │ agent         │ ────────────────────────────────> │ rclone rcd │
│ 配置界面 │ <─────── │ (Python 服务) │ <──────────────────────────────── │  (引擎)    │
└──────────┘          └───────┬───────┘                                   └─────┬──────┘
                              │ 写计划任务                                      │ WinFsp API
                              v                                                v
                      ┌───────────────┐                                   ┌──────────┐
                      │ 计划任务/开机 │                                   │  X: 盘符 │
                      └───────────────┘                                   └──────────┘
```

**职责边界**：agent 不做任何文件系统语义工作。所有「Windows 文件系统语义 ↔ WebDAV 语义」的转换
（缓存、重命名、锁定、断线重连、ACL 映射）都在 rclone 的 `vfs` 层里完成。这正是选 rclone 当引擎的
全部意义——这一层的坑最深，且已经被踩完了。

---

## 2. 关键决策

### D1 · 用 rclone 当引擎（进程级复用），而不是自己实现 WebDAV 客户端 + 文件系统

被否决的方案：Rust/.NET 自研 WebDAV 客户端 + 自研 VFS 语义层。

理由：WebDAV 协议本身不难（PROPFIND / GET / PUT / MOVE / LOCK），真正吃预算的是让
Explorer 满意的那些边缘行为——临时文件模式、`FILE_FLAG_DELETE_ON_CLOSE`、大目录枚举性能、
断线时不要把整个盘符卡死、写回顺序。rclone 的 `vfs` 已经把这些问题解决成一组可调开关
（`--vfs-cache-mode`、`--dir-cache-time`、`--vfs-write-back`），我们用它，就等于继承了这些经验。

### D2 · WinFsp 作为挂载层

- `rclone mount` 在 Windows 上**必须**有 WinFsp（通过 cgofuse 用它的 FUSE 模拟层），不是可选项。
- 备选是 Dokany（RaiDrive 那条路线，MIT/LGPL，授权更宽松）。本次按需求选定 WinFsp。
- ⚠️ **许可证是硬约束**：WinFsp 是 GPLv3 + FLOSS 例外，或商业授权双轨。
  以 FLOSS 方式使用没问题；**要出闭源商业版就必须购买商业授权**。见 README 的许可证章节。

### D3 · 本地 HTTP 服务托管配置界面，而不是桌面框架

- 前端零构建（原生 HTML/JS），改界面不用重编译，也不需要 Node 工具链。
- 顺带获得：可以用浏览器从别的机器访问（默认只绑回环，需要时自行放开并加认证）。
- 代价：需要多一个常驻进程（agent），且要处理「端口被占用」「多实例」这类问题。

### D4 · 全部走 rclone 的 RC API，不直接编辑 rclone.conf，也不起 N 个 `rclone mount` 子进程

- 直接改 conf 文件等于和 rclone 争抢同一个文件的写入权，格式还随版本演进。
- 起 N 个子进程则要自己管进程树、心跳、僵死清理，还拿不到统一的挂载点清单。
- 走 RC：`config/create|update|get|delete` 管配置，`mount/mount|unmount|listmounts` 管挂载，
  **rclone 进程自己是唯一的写入者**，状态查询也是权威的。

代价：RC 端口的权限等价于 shell（官方文档明确说明），所以必须回环 + 认证。
实现上每次启动生成一次性口令（`secrets.token_urlsafe(24)`），只留在 agent 内存里。

### D5 · 开机自启用 Windows 计划任务，不用 WinFsp.Launcher

查证结论（WinFsp `WinFsp-Service-Architecture` 文档）：

- `WinFsp.Launcher` **是一个服务**，但它启动文件系统的方式是**按需唤起**：
  Explorer 映射 `\\name\share` 时，Network Provider 通过命名管道让 Launcher 以
  `-u %1 -m %2`（UNC、挂载点）拉起一个实例；断开时再停掉它。
- 它是「一个可被参数化的实例启动器」，**不是一个开机调度器**，没有「开机把 N 个挂载点全拉起来」的语义。

所以自启由计划任务承担：开机 → 拉起 agent（`--service`）→ agent 逐个 `mount/mount`。

计划任务 XML 里三个必须显式写的值（否则会出难查的故障）：

| 设置 | 值 | 不写会怎样 |
|---|---|---|
| `ExecutionTimeLimit` | `PT0S` | 默认 3 天后任务被强制结束，盘符静默消失 |
| `DisallowStartIfOnBatteries` | `false` | 笔记本拔电源后不启动 |
| `MultipleInstancesPolicy` | `IgnoreNew` | 可能拉起第二个 agent，RC 端口冲突 |

两种模式：`boot`（SYSTEM/开机，全局盘符）/ `logon`（当前用户/登录，仅本会话）。

任务动作会把注册时的运行参数一并固化下来（`--rclone <绝对路径>`、`--data-dir`、`--port`、
`--rc-port`、`--web-dir`，必要时还有 `--host` / `--allow-remote`）。不这么做的话，
手工用 `--rclone D:\tools\rclone.exe` 跑通、重启后任务却找不到 rclone，是一种很难查的
「上次好好的，重启就不挂载」。参数经 `subprocess.list2cmdline()` 序列化，带空格的路径
（如 `C:\My Data`）不会被 Task Scheduler 按空格拆开。

### D6 · agent 用 Python 标准库，零第三方依赖

目标机是 Windows，装 Python 已经是门槛，pip 依赖会让「下载即可运行」变成「配环境」。
所以只用 `http.server` / `urllib` / `ctypes` / `subprocess`，整个 agent 可直接拷走运行。

### D7 · 密码用 DPAPI（机器范围）

必须机器范围（`CRYPTPROTECT_LOCAL_MACHINE`），因为 `boot` 模式下 agent 以 SYSTEM 运行，
需要无交互解密。Per-user 范围更安全但会让无人值守开机挂载失败。

明确的取舍：机器范围 DPAPI 的官方语义是**同机任意用户都能解密**（`CryptUnprotectData` 不需要
管理员权限），不只是管理员。所以只靠 DPAPI 还不够：

1. agent 启动时对数据目录执行 ACL 收紧（去掉 `%PROGRAMDATA%` 继承来的 `Users:(OI)(CI)(RX)`，
   只保留 `SYSTEM` / `Administrators` / 当前运行账户），日志会记录结果；
2. 若威胁模型包含本机其他用户，DPAPI 本身就不够了，需要换成外部密钥库
   （Windows Credential Manager + 自定义服务账户，或企业 KMS）。

实现注意：`icacls` 的 `(OI)(CI)` 继承标记只对目录有效，和 `/T` 一起用会让子文件变成空 DACL
（连属主都读不了）。所以 `harden_data_dir()` 分两遍：先用不带继承标记的显式授权修所有子对象，
再单独给目录设置可继承授权。

### D8 · rclone.conf 静态加密，密钥交给 DPAPI

rclone 里密码只是 **obscure**——一个可逆的混淆，不是加密。只要 rclone.conf 可读，
WebDAV 密码就能还原。这会让 D7 做的 DPAPI 保护形同虚设（相当于从后门把密码又摊开了）。

做法：

1. 首次启动生成一个随机配置口令，DPAPI 保护后存 `config_key.enc`；
2. `agent/pwcmd.py` 作为 rclone 的 `--password-command`，被调用时解密并输出该口令；
3. 配置尚未加密时，先执行 `rclone config encryption set --password-command ...` 迁移，
   再启动 `rclone rcd`。

已在 rclone v1.75.1 / Windows 11 上实测的关键点：

- 加密后的配置以 `# Encrypted rclone configuration File` 开头，remote 名与口令都不出现在明文里；
- 不带 `--password-command` 时读取直接失败（rc=1），说明密钥确实在起作用；
- `rclone rcd` 能读取**并且能通过 RC 向加密配置写入新 remote**，写完仍是加密状态——
  这一点很关键，否则 agent 每加一个连接就会把配置写回明文。

⚠️ 一个易错点：`RCLONE_CONFIG_PASS` 环境变量**不能**用来非交互地设置加密口令
（实测退化为交互式提示并因 EOF 失败）。非交互入口只有 `--password-command`。

⚠️ 另一个实现细节：全新的 rclone.conf 是 0 字节文件，**必须在 rclone 写入第一个 remote 之前
就加密**。早期实现里迁移步骤对空文件直接 return，导致整个会话创建的 remote 都以明文落盘。

⚠️ 密钥丢失的处理：`config_key.enc` 不存在但 `rclone.conf` 仍加密时，agent 必须**拒绝启动**。
否则会重新生成一把新钥匙，而 `rclone rcd` 在启动阶段不读配置（`core/version` 能通），
错误会推迟到第一次配置操作，表现为 rclone 内部 panic，极难定位。

### D9 · 本地 HTTP API 的信任边界：回环 ≠ 可信

「只绑 127.0.0.1 所以只有本机用户能访问」这个前提对浏览器不成立：

- **DNS rebinding**：恶意页面把自己的域名解析到 `127.0.0.1`，对浏览器而言就是同源，
  可以完整读写 agent 的 API；
- **CSRF**：用 `<form>` / `fetch` 加 `Content-Type: text/plain` 发简单请求不触发预检，
  如果服务端不校验 Content-Type，body 又恰好是合法 JSON，就会执行写操作
  （建/改连接、挂载攻击者的 WebDAV、清空密码、增删自启任务）。

因此 `Handler` 在路由之前做三件事：

1. `Host` 必须命中本机绑定地址/回环别名（挡 DNS rebinding）；
2. 带 `Origin` 的请求必须与 `Host` 同源（挡跨站请求）；
3. POST 只接受 `application/json`（挡简单请求 CSRF），并对 body 大小设上限。

配套：profile id 在 `ProfileStore.upsert` 里限制为 `[A-Za-z0-9_-]{1,64}`，前端渲染时转义——
id 会进 `data-id` 属性，不校验就是一个存储型 XSS。

注意这挡不住**同机的其他本地用户**（他们可以不带 Origin 直接 curl）；多用户机器需要额外的
认证层。另外 `--host` 绑非回环地址现在必须显式 `--allow-remote`，因为那等于把无鉴权 API
暴露到网络。

---

## 3. 权限与会话：为什么自启默认是「登录」模式

这是整个设计里最容易出「明明挂上了却看不见 / 写不了」的地方。

| 挂载者身份 | 盘符可见范围 | 备注 |
|---|---|---|
| 普通用户（非提权） | 该用户自己的会话，Explorer 可见 | **默认推荐**，即 `logon` 模式 |
| 管理员（UAC 提权） | **其他账户看不到，Explorer 也看不到** | 反直觉：提权反而更不可见。不要用「以管理员身份运行」来挂盘 |
| SYSTEM | 全局，所有用户可见 | `boot` 模式走这条路，代价见下 |

`boot` 模式的两个真实代价：

1. **属主变成 SYSTEM。** 其他账户只有 group/others 权限，而 rclone 在 Windows 上把
   group/others 的「可写」映射为 write attributes / write data / append data，
   **不含 write extended attributes**。需要写扩展属性的程序会失败。
   缓解手段是直接给 FUSE 层指定安全描述符，在「附加参数」里加：
   `--fuse-flag FileSecurity=D:P(A;;FRFW;;;WD)`
2. **凭据暴露面变大。** SYSTEM 能解密的密钥，本机管理员也能解密（见 D7）。机器范围 DPAPI
   实际是同机任意用户可解密，所以数据目录还额外做了 ACL 收紧。

因此默认值是 `logon`。只有「必须无人值守 + 需要多用户可见」时才切 `boot`，
并按上面两条补好权限映射。

还有一条退路：挂到**不存在的子目录路径**（如 `C:\mnt\nas`）而不是盘符，
可以绕开盘符在提权会话中的可见性问题。`build_mount_params()` 现在接受 `X:` / `*` /
绝对目录路径；`--network-mode` 下 rclone 只支持盘符，所以目录挂载点与网络模式互斥。

---

## 4. 界面字段 → rclone RC 参数映射

| 界面字段 | RC 参数 | 备注 |
|---|---|---|
| 盘符或目录 | `mountPoint` | 单个字母加冒号、`*`（自动分配），或绝对目录路径（如 `C:\mnt\nas`） |
| WebDAV 地址 / 账号 / 密码 / 服务端类型 | `config/create` 的 `parameters.url/user/pass/vendor` | `opt.obscure=true` 让 rclone 自己混淆密码 |
| 卷标 | `mountOpt.VolumeName` | 网络模式下会变成共享名 |
| 网络驱动器 | `network_mode`（扁平参数） | 等价 `--network-mode`；此模式下不支持目录挂载点 |
| 只读 | `read_only`（扁平参数） | |
| VFS 缓存模式 | `vfsOpt.CacheMode` | `off` / `minimal` / `writes` / `full` |
| 目录缓存时间 | `vfsOpt.DirCacheTime` | 如 `5m`、`1h` |
| 附加参数 | 扁平参数（键名 = CLI flag 去掉 `--`、`-` 换 `_`） | 高级用户可透传官方 flag；`password_command` 等进程级 flag 被黑名单拒绝 |

扁平参数与嵌套块的规则（来自 rclone 文档）：两者都传时**嵌套块优先**。
实现见 `agent/server.py` 的 `build_mount_params()` 与 `parse_extra_opts()`。

---

## 5. 待在你的机器上验证的清单

这些点我无法在文档层面替你确认，第一次跑请按顺序过一遍：

1. **WinFsp 装上后 `rclone mount` 真能工作**
   `bin\rclone.exe mount :webdav,url=...,vendor=other,user=...,pass=...: X:` 手工试一次。
2. **权限与会话的实际表现**（先 `logon`，再 `boot`）
   - `logon` 模式挂上后，当前用户 Explorer 里可见 → 建立基线
   - 故意用「以管理员身份运行」跑一次 agent 再挂载，确认盘符在普通会话里**变得不可见**
     （验证第 3 节那条反直觉结论）
   - 切 `boot`（SYSTEM）模式确认全局可见；然后用**非管理员账户**往盘里写文件，
     确认是否需要补 `--fuse-flag FileSecurity=...`
3. **中文文件名 / 超长路径 / Emoji**
4. **大目录（>5000 项）首次枚举耗时**，据此调 `dir_cache_time`
5. **Office 打开并保存**，确认 `vfs_cache_mode` 够用
6. **断网再恢复**后盘符是否还能用（rclone 会重连，但要确认没有永久卡死的句柄）
7. **重启后**计划任务是否真的把它挂起来了（`schtasks /Query /TN WebDavDrive-Agent /V /FO LIST`）；
   顺便确认任务参数里带着 `--rclone <绝对路径>`、`--data-dir`、`--rc-port`
8. **多用户机器上的凭据隔离**：用另一个本地账户尝试读取
   `%PROGRAMDATA%\WebDavDrive\profiles.json` / `config_key.enc`，应当被拒绝；
   再检查 `agent.log` 里是否有「数据目录权限已收紧」
9. **目录挂载点**：把挂载点改成 `C:\mnt\nas`（目录可不存在），确认资源管理器里能打开，
   且 `--network-mode` 下会被明确拒绝

---

## 6. 演进路线

| 阶段 | 内容 |
|---|---|
| 已完成（0.1） | 单 agent + RC 编排 + Web 配置 + 计划任务自启 + rclone.conf 静态加密 |
| 已完成（0.1.1） | 安全加固：Host/Origin/Content-Type 校验、id 校验与前端转义、数据目录 ACL、密钥丢失拒启、密码解密失败不静默覆盖；健壮性：Job Object 回收子进程、profiles.json 隔离备份、自启参数固化与引号、目录挂载点、`--allow-remote` 显式开关 |
| 0.2 | 打包成单 exe（PyInstaller）+ 把 agent 装成真正的 Windows 服务（NSSM 或 pywin32），去掉 Python 前置依赖 |
| 0.3 | 挂载健康检查与自动重挂（断线后自愈）、带宽/缓存占用可视化 |
| 0.4 | 可选注册 WinFsp Launcher 服务项，让 `net use \\name\share X:` 也能按需挂载 |
| 0.5 | 多用户/多站点、凭据走 Credential Manager、配置导入导出 |
