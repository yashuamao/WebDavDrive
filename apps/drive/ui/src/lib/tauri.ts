import { invoke } from "@tauri-apps/api/core";
import type {
  AppStatus,
  AutostartStatus,
  ConnectionInput,
  ConnectionView,
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
  exitApplication: () => invoke<void>("exit_application"),
  forceExit: () => invoke<void>("force_exit"),
};
