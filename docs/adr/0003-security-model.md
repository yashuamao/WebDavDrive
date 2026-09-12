# ADR-0003 安全模型

- 状态：已接受（2026-09-12）
- 背景：旧版在安全审查中发现并修复了若干真实问题，新架构必须继承结论。

## 决定

**凭据**

- 远端密码用机器范围 DPAPI 保护（boot/SYSTEM 需要无交互解密）；
- 引擎配置文件静态加密，密钥由 DPAPI 保护，经 `--password-command` 提供；
- 密钥丢失/损坏而配置仍加密 → 拒绝启动，不静默重建；
- 解密失败不得用空值覆盖引擎现有凭据；
- 文档必须写明：机器范围 DPAPI 对本机**任意用户**可解密，不是仅管理员。

**数据目录**

- 启动时收紧 ACL：去掉继承，仅 SYSTEM / Administrators / 当前运行账户；
- `profiles.json` 损坏隔离备份，不静默清空。

**进程与端口**

- 引擎 RC 只绑回环，口令每次启动随机生成、经环境变量传递、不进 argv/不落盘；
- 引擎进程放入 `KILL_ON_JOB_CLOSE` Job Object，宿主被强杀时内核回收。

**外部输入**

- 附加参数黑名单：`password_command`、`config`、`rc_*`、`log_file` 等进程级 flag；
- 路径类输入必须 canonical 校验在允许根内，拒绝符号链接/junction 越界；
- 若提供本地 Web 管理页：Host 白名单 + Origin 同源 + 仅 `application/json` + body 上限；
- 日志不输出密码/密钥明文。

## 后果

- `apps/drive` 的 IPC 命令必须结构化参数，禁止任意路径/任意命令；
- `foundation-server`（后续）内置上述 HTTP 规则，Koma Phase 5 直接复用；
- 多用户机器上的凭据隔离仍依赖系统账户与文件 ACL，不承诺抵御本机管理员。
