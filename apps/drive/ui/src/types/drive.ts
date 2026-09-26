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

/** 引擎（rclone）更新状态；网络失败只体现在 last_error 上，不弹窗。 */
export interface EngineUpdateInfo {
  installed_version: string | null;
  latest_version: string | null;
  update_available: boolean;
  /** 上次检查时间（Unix 秒）。 */
  last_check_at: number | null;
  last_error: string | null;
  notes: string | null;
  /** 空串 = rclone 官方源。 */
  mirror_prefix: string;
  source: string;
  /** 本次是否真的发起了网络检查（被每天一次的限制跳过时为 false）。 */
  checked: boolean;
  not_modified: boolean;
}

/** 应用（安装包）自更新状态；网络失败只体现在 last_error 上，不弹窗。 */
export interface AppUpdateInfo {
  installed_version: string;
  latest_version: string | null;
  update_available: boolean;
  /** 上次检查时间（Unix 秒）。 */
  last_check_at: number | null;
  last_error: string | null;
  notes: string | null;
  source: string;
  /** 本次是否真的发起了网络检查（被每天一次的限制跳过时为 false）。 */
  checked: boolean;
  not_modified: boolean;
  /** 绿色版（免安装 zip）不支持自动更新，只能去 GitHub 下载。 */
  portable: boolean;
  /** 安装包是否已下载并校验通过，可以直接安装。 */
  setup_downloaded: boolean;
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
