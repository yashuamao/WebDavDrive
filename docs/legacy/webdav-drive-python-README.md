# WebDAV Drive

把服务器上的 WebDAV 挂成 Windows 盘符的小工具。引擎用 **rclone**，挂载层用 **WinFsp**，
上面套一个本地 Web 配置界面（连接 / 账号密码 / 盘符 / 开机自动挂载）。

```
浏览器 ──HTTP──> 本地 agent ──RC API──> rclone rcd ──WinFsp──> X:  （资源管理器可见）
```

agent 不是文件系统，它只做三件事：托管配置页、把配置翻译成 rclone 调用、按需在开机时把挂载拉起来。

---

## 为什么是这个组合

| 层 | 选型 | 理由 |
|---|---|---|
| 挂载 | **WinFsp** | Windows 上的 FUSE 模拟层，`rclone mount` 在 Windows 的硬依赖（经 cgofuse）。成熟度高，rclone、大量商业产品都在用。 |
| 引擎 | **rclone** | WebDAV 只是它 40+ 个 backend 之一，`vfs` + `webdav` 是这件事的现成教科书实现。MIT。 |
| 配置界面 | 本地 Web 页 | 跨机器可用、改 UI 不用重编译、不需要前端构建链。 |
| 自启 | Windows 计划任务 | WinFsp.Launcher 是**按需唤起**的，不是开机调度器，见下。 |

**为什么不用 WinFsp.Launcher 做开机自启**：Launcher 确实是常驻服务，但它的职责是
「Explorer 映射 `\\name\share` 时按需拉起一个文件系统实例」（命令行长这样：`-u %1 -m %2`），
并不负责在开机时把 N 个挂载点全部拉起来。所以自启交给计划任务，由计划任务在开机时拉起 agent，
agent 再逐个 mount。相关签名见 WinFsp 的 `WinFsp-Service-Architecture` 文档。

---

## 快速开始

前置：Windows 10/11、Python 3.10+、管理员权限（仅自启需要）。

```powershell
# 1) 装依赖：检查 WinFsp，下载 rclone.exe 到 bin\
.\scripts\bootstrap.ps1

# 2) 起 agent（首次建议用管理员 PowerShell，方便之后注册自启）
cd webdav-drive
python -m agent.server
```

浏览器会自动打开 `http://127.0.0.1:8787/`。在页面里：

1. 填 **WebDAV 地址**（要填到能直接列目录那一层，通常以 `/dav`、`/remote.php/dav` 结尾）
2. 填账号密码 →  **保存**
3. 点 **测试连接** 确认能列目录
4. 点 **挂载** → 资源管理器里出现盘符
5. 在「开机自动挂载」里 **注册** → 重启后自动挂上

---

## 目录结构

```
webdav-drive/
├── agent/                  # Python 标准库实现的本地服务，零第三方依赖
│   ├── server.py           # HTTP 服务 + rclone 进程托管 + mount/unmount 编排
│   ├── rclone_rc.py        # rclone RC API 客户端
│   ├── profiles.py         # 连接配置存储（密码走 DPAPI 加密）
│   ├── pwcmd.py            # rclone --password-command：输出配置加密口令
│   ├── autostart.py        # 计划任务注册（boot / logon 两种模式）
│   └── winenv.py           # WinFsp / rclone 探测 + DPAPI 封装
├── web/                    # 配置界面（原生 HTML/JS，无构建步骤）
├── tests/                  # 测试（见下）
├── scripts/bootstrap.ps1   # 环境准备
├── CHANGELOG.md            # 每个版本改了什么、为什么、怎么验证
└── docs/architecture.md    # 设计决策、许可证分析、验证清单
```

## 测试

```powershell
python tests/smoke_test.py        # 94 项：DPAPI、参数映射、HTTP API（假 rclone）、API 信任边界、配置存储自愈
python tests/integration_test.py  # 20 项：真 rclone 进程 + 真 RC API + 配置加密 + RC 认证
```

`integration_test.py` 不需要 WinFsp，也不需要可用的 WebDAV 服务器——它会**故意**触发一次
挂载失败，验证错误被干净地上报、且 agent 不会被拖死。

---

## 命令行参数

| 参数 | 说明 |
|---|---|
| `--port 8787` | 配置界面端口 |
| `--data-dir <路径>` | 配置/日志目录，默认 `%PROGRAMDATA%\WebDavDrive` |
| `--rclone <路径>` | 指定 rclone.exe，默认找 `bin\` 和 PATH |
| `--rc-port 5572` | 内部 rclone RC 端口 |
| `--attach 127.0.0.1:5572 --rc-user u --rc-pass p` | 附加到已有 rclone，不自己拉起 |
| `--service` | 计划任务用的模式：静默启动并先挂载所有自启项 |
| `--allow-remote` | 显式允许绑定非回环 `--host`（API 无鉴权，默认拒绝，见安全说明） |

---

## 开机自动挂载的两种模式

界面里默认选 **「登录时」**。原因是盘符可见性跟挂载者身份强相关，而且有个反直觉的坑：

| 挂载者身份 | 盘符可见范围 | 说明 |
|---|---|---|
| 普通用户（非提权） | 该用户会话，Explorer 可见 | ✅ 默认推荐，对应 `logon` 模式 |
| 管理员（UAC 提权） | **其他账户看不到，Explorer 也看不到** | ⚠️ 提权反而更不可见——不要用「以管理员身份运行」来挂盘 |
| SYSTEM | 全局，所有用户可见 | 对应 `boot` 模式，但有下面两个代价 |

`boot` 模式的两个真实代价：

1. **属主变成 SYSTEM**，其他账户只有 group/others 权限，而 rclone 在 Windows 上把
   group/others 的可写映射为 write data / append data，**不含 write extended attributes**，
   需要写扩展属性的程序会失败。缓解办法是在「附加参数」里加
   `--fuse-flag FileSecurity=D:P(A;;FRFW;;;WD)`。
2. **凭据暴露面变大**：SYSTEM 能解密的东西，本机**任意用户**都能解密（机器范围 DPAPI 的语义，
   详见安全说明），不只是管理员。

所以：个人机用 `logon`；只有「必须无人值守 + 多用户可见」才用 `boot`，并把上面的权限映射补上。
两种模式注册时都需要管理员权限（写计划任务需要）。

如果连 `logon` 下盘符可见性也有问题，退路是挂到**不存在的子目录路径**（如 `C:\mnt\nas`）
而不是盘符，可以绕开盘符在提权会话里的可见性问题。界面上的「盘符或目录」字段同时接受
`X:`、`*` 和 `C:\mnt\nas` 这类绝对目录（网络驱动器模式下只接受盘符）。

---

## 安全说明（别跳过）

- **RC 端口 = shell 权限。** rclone 官方文档明确写了这一点（`core/command` 可以执行任意命令，
  `config/dump` 能吐出所有凭据）。因此 agent 只绑 `127.0.0.1`，RC 只绑回环，
  且**每次启动重新生成一次性口令**，口令通过 `RCLONE_RC_PASS` 环境变量传给 rclone（不进程命令行）。
  非回环绑定现在会被拒绝，必须显式加 `--allow-remote`；而 API 没有认证，放出去等于把控制权交给网络。
- **本地 API 有信任边界。** 只绑回环并不够：浏览器里的恶意页面可以（a）用 `text/plain` 简单请求
  向 127.0.0.1 发跨站 POST，（b）通过 DNS rebinding 把自己的域名解析到 127.0.0.1 后同源读写。
  所以 agent 现在会校验 `Host` 必须指向本机、拒绝跨站 `Origin`、且 POST 只接受
  `Content-Type: application/json`；配置 id 也限制为安全字符，前端渲染时转义。
- **密码不再以可还原形式落盘。** rclone 里的密码只是 **obscure**（可逆混淆，不是加密），
  所以只把密码放进 rclone.conf 等于没保护。本工具的做法：
  - WebDAV 密码用 DPAPI（机器范围）加密后存进 `profiles.json`；
  - rclone 配置本身用 rclone 的配置加密功能加密，密钥同样由 DPAPI 保护（`config_key.enc`），
    通过 `--password-command` 在 rclone 需要时解密喂给它；
  - 结果是 `rclone.conf` 里看不到 remote 名，也看不到任何口令。
  - DPAPI 用机器范围是必然选择：`boot` 模式下 agent 以 SYSTEM 运行，需要无交互解密。
    **代价要比「管理员可解密」更大：机器范围 DPAPI 的官方语义是同机任意用户都能解密。**
    因此 agent 启动时会把数据目录的 ACL 收紧为仅 `SYSTEM` / `Administrators` / 当前运行账户
    （日志里能看到「数据目录权限已收紧」）；这一步失败会在日志里报 ERROR。
    若威胁模型包含本机其他用户，应该换成外部密钥库（Credential Manager / KMS）而不是依赖 DPAPI。
  - `config_key.enc` 丢失而 `rclone.conf` 仍加密时，agent 会**拒绝启动并明确报错**，不会静默
    重建密钥（那会让旧配置永远解不开，且错误会推迟到第一次配置操作才暴露）。
- **配置界面本身没有登录。** 回环 + Host/Origin 校验能挡住浏览器跨站攻击，但**同一台机器上的
  其他本地用户**仍然能直接访问 127.0.0.1 的 API。多用户机器上如果这不够，需要再套一层认证。

---

## 许可证

- **rclone**：MIT —— 可自由复用、可参考其实现。
- **WinFsp**：GPLv3 + FLOSS 例外，或商业授权双轨。
  作为 FLOSS 项目使用没有问题；**如果你打算发布闭源/商业版本，需要购买 WinFsp 商业授权**。
  这是架构前提，不是法务细节——请在开始包装产品之前就定下来。
- 本工具只是**以独立进程方式调用** rclone.exe，并依赖系统已安装的 WinFsp，
  没有链接它们的代码到自己的二进制里。

---

## 排错

| 现象 | 原因 / 处理 |
|---|---|
| 挂载报 WinFsp 相关错误 | 没装 WinFsp，或装的架构不对。跑一遍 `bootstrap.ps1` 看检测结果。 |
| 资源管理器里看不到盘符 | 若 agent 以 SYSTEM 跑，盘符是全局的；先按 `boot` 模式验证。看不到就先换 `logon` 模式排除会话问题，详见 `docs/architecture.md` 的验证清单。 |
| Office / 剪辑软件打开文件报错 | VFS 缓存模式设成 `writes` 或 `full`（它们要写临时文件再改名）。 |
| 大目录打开很慢 | 加大 `dir_cache_time`（如 `5m` → `1h`），或加 `--vfs-cache-mode full`。 |
| 端口 8787 被占用 | 已有 agent 在跑，或换 `--port`。 |
| 中文文件名乱码 | 检查 WebDAV 服务端编码；可加 `--no-check-certificate`（自签证书）、`--header` 等透传参数在「附加参数」里。 |
| 别的账户或 Explorer 看不到盘符 | 多半是以「管理员身份」跑的 agent。改成普通用户身份运行，自启用「登录时」模式；或把挂载点改成目录（如 `C:\mnt\nas`）。 |
| SYSTEM 模式下其他账户写不进文件 | rclone 默认权限映射缺 write extended attributes。在「附加参数」里加 `--fuse-flag FileSecurity=D:P(A;;FRFW;;;WD)`。 |
| 配置密钥损坏 / 换过机器账户 | `config_key.enc` 解不开时 agent 会明确报错而不是静默重建。删除 `config_key.enc` 与 `rclone.conf` 后重新添加连接。 |
| 删配置报 502「无法从 rclone 删除 remote」 | rclone 不可达时故意保留配置，避免留下带凭据的孤儿 remote。等 rclone 恢复后重试删除。 |
| 提示「拒绝非本机 Host」 | 你用了别的名字/IP 访问界面，或浏览器里是攻击页面。用 `http://127.0.0.1:8787/` 或 `http://localhost:8787/`。 |
| 重启后不挂载，日志里有「找不到 rclone.exe」 | 自启任务会带上注册时的 `--rclone` 绝对路径；若移动过 rclone，重新注册一次自启。 |
