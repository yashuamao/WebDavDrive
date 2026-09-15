import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { useToast } from "@/components/feedback/ToastProvider";
import { driveApi } from "@/lib/tauri";
import { errorMessage } from "@/lib/utils";
import type {
  AppStatus,
  AutostartStatus,
  ConnectionInput,
  ConnectionView,
  DriveAction,
  OperationState,
} from "@/types/drive";
import { emptyStatus } from "@/types/drive";

const operationLabels: Record<DriveAction, string> = {
  mount: "正在挂载…",
  unmount: "正在卸载…",
  open: "正在打开…",
  probe: "正在验证…",
};

export function useDriveManager() {
  const { showToast } = useToast();
  const [status, setStatus] = useState<AppStatus>(emptyStatus);
  const [profiles, setProfiles] = useState<ConnectionView[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [operations, setOperations] = useState<Record<string, OperationState | undefined>>({});
  const [errors, setErrors] = useState<Record<string, string | undefined>>({});
  const [refreshing, setRefreshing] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [autostart, setAutostart] = useState<AutostartStatus | null>(null);
  const [autostartBusy, setAutostartBusy] = useState(false);
  const [autostartError, setAutostartError] = useState("");
  const [logs, setLogs] = useState<string[]>([]);
  const [logsLoading, setLogsLoading] = useState(false);
  const [logsError, setLogsError] = useState("");
  const [exitError, setExitError] = useState("");
  const [exitBusy, setExitBusy] = useState(false);

  const refresh = useCallback(async (notify = false) => {
    setRefreshing(true);
    try {
      const [nextStatus, nextProfiles] = await Promise.all([
        driveApi.status(),
        driveApi.listConnections(),
      ]);
      setStatus(nextStatus);
      setProfiles(nextProfiles);
      setSelectedId((current) =>
        nextProfiles.some((profile) => profile.id === current)
          ? current
          : nextProfiles[0]?.id ?? null,
      );
      setLoadError("");
      if (notify) showToast("状态已刷新");
    } catch (error) {
      const message = errorMessage(error);
      setLoadError(message);
      if (notify) showToast(`刷新失败：${message}`, "danger");
      throw error;
    } finally {
      setRefreshing(false);
    }
  }, [showToast]);

  const refreshAutostart = useCallback(async () => {
    try {
      const next = await driveApi.autostartStatus();
      setAutostart(next);
      setAutostartError("");
      return next;
    } catch (error) {
      setAutostartError(errorMessage(error));
      return null;
    }
  }, []);

  const loadLogs = useCallback(async (notify = false) => {
    setLogsLoading(true);
    setLogsError("");
    try {
      const next = await driveApi.logs();
      setLogs(next);
      if (notify) showToast("日志已刷新");
    } catch (error) {
      setLogsError(errorMessage(error));
    } finally {
      setLogsLoading(false);
    }
  }, [showToast]);

  useEffect(() => {
    void refresh().catch(() => {});
    void refreshAutostart();
    const interval = window.setInterval(() => {
      if (!document.hidden) void refresh().catch(() => {});
    }, 5000);
    return () => window.clearInterval(interval);
  }, [refresh, refreshAutostart]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    listen<string>("exit-cleanup-failed", (event) => {
      setExitBusy(false);
      setExitError(String(event.payload || "请关闭正在使用虚拟硬盘的程序后重试。"));
      showToast("退出已中止：部分虚拟硬盘未能卸载", "danger");
      void refresh().catch(() => {});
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch((error) => {
      console.warn("无法订阅退出清理失败事件：", error);
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refresh, showToast]);

  const runAction = useCallback(async (id: string, action: DriveAction) => {
    setOperations((current) => ({ ...current, [id]: { action, label: operationLabels[action] } }));
    setErrors((current) => ({ ...current, [id]: undefined }));
    try {
      if (action === "mount") {
        await driveApi.mountConnection(id);
        showToast("驱动器已挂载");
      } else if (action === "unmount") {
        await driveApi.unmountConnection(id);
        showToast("驱动器已卸载");
      } else if (action === "open") {
        await driveApi.openConnection(id);
      } else {
        const report = await driveApi.probeConnection(id);
        const sample = report.sample.join("、");
        showToast(`连接成功，根目录 ${report.entries} 项${sample ? `：${sample}` : ""}`);
      }
    } catch (error) {
      setErrors((current) => ({ ...current, [id]: errorMessage(error) }));
    } finally {
      setOperations((current) => ({ ...current, [id]: undefined }));
      if (action !== "open") void refresh().catch(() => {});
    }
  }, [refresh, showToast]);

  const saveConnection = useCallback(async (input: ConnectionInput) => {
    const saved = await driveApi.saveConnection(input);
    setSelectedId(saved.id);
    await refresh();
    showToast("连接已保存");
  }, [refresh, showToast]);

  const deleteConnection = useCallback(async (profile: ConnectionView) => {
    try {
      await driveApi.deleteConnection(profile.id);
      setSelectedId(null);
      await refresh();
      showToast("连接已删除");
      return true;
    } catch (error) {
      setErrors((current) => ({ ...current, [profile.id]: errorMessage(error) }));
      return false;
    }
  }, [refresh, showToast]);

  const installAutostart = useCallback(async (mode: "logon" | "boot") => {
    setAutostartBusy(true);
    setAutostartError("");
    try {
      const next = await driveApi.installAutostart(mode);
      setAutostart(next);
      showToast("自动挂载任务已注册");
    } catch (error) {
      setAutostartError(errorMessage(error));
    } finally {
      setAutostartBusy(false);
    }
  }, [showToast]);

  const uninstallAutostart = useCallback(async () => {
    setAutostartBusy(true);
    setAutostartError("");
    try {
      const next = await driveApi.uninstallAutostart();
      setAutostart(next);
      showToast("自动挂载任务已移除");
    } catch (error) {
      setAutostartError(errorMessage(error));
    } finally {
      setAutostartBusy(false);
    }
  }, [showToast]);

  const retryExit = useCallback(async () => {
    setExitBusy(true);
    setExitError("");
    try {
      await driveApi.exitApplication();
    } catch (error) {
      setExitBusy(false);
      setExitError(errorMessage(error));
    }
  }, []);

  const forceExit = useCallback(async () => {
    setExitBusy(true);
    try {
      await driveApi.forceExit();
    } catch (error) {
      setExitBusy(false);
      setExitError(errorMessage(error));
    }
  }, []);

  return {
    status,
    profiles,
    selectedId,
    operations,
    errors,
    refreshing,
    loadError,
    autostart,
    autostartBusy,
    autostartError,
    logs,
    logsLoading,
    logsError,
    exitError,
    exitBusy,
    setSelectedId,
    refresh,
    refreshAutostart,
    loadLogs,
    runAction,
    saveConnection,
    deleteConnection,
    installAutostart,
    uninstallAutostart,
    retryExit,
    forceExit,
  };
}
