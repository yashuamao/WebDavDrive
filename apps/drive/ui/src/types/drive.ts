export type ViewName = "drives" | "autostart" | "logs" | "settings";
export type ThemeMode = "system" | "light" | "dark";
export type DriveAction = "mount" | "unmount" | "open" | "probe";
export type StatusTone = "success" | "neutral" | "warning" | "danger" | "busy";

export interface ConnectionView {
  id: string;
  name: string;
  remote: string;
  url: string;
  vendor: string;
  user: string;
  password_set: boolean;
  drive: string;
  volname: string;
  network_mode: boolean;
  vfs_cache_mode: string;
  dir_cache_time: string;
  read_only: boolean;
  autostart: boolean;
  extra_opts: string;
}

export interface ConnectionInput {
  id: string | null;
  name: string;
  url: string;
  vendor: string;
  user: string;
  password: string;
  clear_password: boolean;
  drive: string;
  volname: string;
  network_mode: boolean;
  vfs_cache_mode: string;
  dir_cache_time: string;
  read_only: boolean;
  autostart: boolean;
  extra_opts: string;
}

export interface EngineStatus {
  installed: boolean;
  path: string | null;
  version: string | null;
  running: boolean;
  rc_addr: string | null;
}

export interface MountRecord {
  fs: string;
  mount_point: string;
}

export interface AppStatus {
  engine: EngineStatus;
  mounts: MountRecord[];
  profile_count: number;
  secrets: string;
}

export interface AutostartStatus {
  installed: boolean;
  mode: "logon" | "boot" | null;
  task_name: string | null;
}

export interface ProbeReport {
  entries: number;
  sample: string[];
}

export interface OperationState {
  action: DriveAction;
  label: string;
}

export const emptyStatus: AppStatus = {
  engine: { installed: false, path: null, version: null, running: false, rc_addr: null },
  mounts: [],
  profile_count: 0,
  secrets: "-",
};
