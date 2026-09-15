import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import type { ConnectionView, MountRecord, StatusTone } from "@/types/drive";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

export function mountFor(profile: ConnectionView, mounts: MountRecord[]): MountRecord | null {
  const configuredPoint = profile.drive.toLocaleLowerCase();
  const configuredRemote = `${profile.remote}:`.toLocaleLowerCase();
  return mounts.find((mount) =>
    mount.mount_point.toLocaleLowerCase() === configuredPoint
      || mount.fs.toLocaleLowerCase() === configuredRemote,
  ) ?? null;
}

export function displayDrive(profile: ConnectionView, mount: MountRecord | null): string {
  const point = mount?.mount_point || profile.drive || "*";
  const match = /^([a-z]):/i.exec(point);
  return match ? `${match[1].toUpperCase()}:` : "DAV";
}

export function connectionStatus(
  _profile: ConnectionView,
  mount: MountRecord | null,
  pendingLabel?: string,
  hasError = false,
): { label: string; tone: StatusTone } {
  if (pendingLabel) return { label: pendingLabel, tone: "busy" };
  if (mount) return { label: "已挂载", tone: "success" };
  if (hasError) return { label: "操作失败", tone: "danger" };
  return { label: "未连接", tone: "neutral" };
}
