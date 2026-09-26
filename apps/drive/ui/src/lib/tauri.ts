import { invoke } from "@tauri-apps/api/core";
import type {
  AppStatus,
  AutostartStatus,
  ConnectionInput,
  ConnectionView,
  EngineUpdateInfo,
  MountRecord,
  ProbeReport,
} from "@/types/drive";

export const driveApi = {
  status: () => invoke<AppStatus>("app_status"),
  listConnections: () => invoke<ConnectionView[]>("list_connections"),
  saveConnection: (input: ConnectionInput) =>
    invoke<ConnectionView>("save_connection", { input }),
  deleteConnection: (id: string) => invoke<void>("delete_connection", { id }),
  probeConnection: (id: string) => invoke<ProbeReport>("probe_connection", { id }),
  mountConnection: (id: string) => invoke<MountRecord>("mount_connection", { id }),
  unmountConnection: (id: string) => invoke<void>("unmount_connection", { id }),
  openConnection: (id: string) => invoke<void>("open_connection", { id }),
  logs: (limit = 500) => invoke<string[]>("logs", { limit }),
  autostartStatus: () => invoke<AutostartStatus>("autostart_status"),
  installAutostart: (mode: "logon" | "boot") =>
    invoke<AutostartStatus>("install_autostart", { mode }),
  uninstallAutostart: () => invoke<AutostartStatus>("uninstall_autostart"),
  engineStatus: () => invoke<EngineUpdateInfo>("engine_status"),
  // force = 设置页手动按钮：随时可点，绕过"自动检查每天最多一次"的限制。
  checkEngineUpdate: (force: boolean) => invoke<EngineUpdateInfo>("check_engine_update", { force }),
  setEngineMirrorPrefix: (prefix: string) =>
    invoke<EngineUpdateInfo>("set_engine_mirror_prefix", { prefix }),
  installEngineUpdate: (version: string) =>
    invoke<EngineUpdateInfo>("install_engine_update", { version }),
  installEngineFromFile: (path: string) =>
    invoke<EngineUpdateInfo>("install_engine_from_file", { path }),
  pickEngineFile: () => invoke<string | null>("pick_engine_file"),
  exitApplication: () => invoke<void>("exit_application"),
  forceExit: () => invoke<void>("force_exit"),
};
