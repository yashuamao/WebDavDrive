import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect, useState } from "react";
import { ConnectionDialog } from "@/components/drive/ConnectionDialog";
import { useToast } from "@/components/feedback/ToastProvider";
import { AppLayout } from "@/components/shell/AppLayout";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useDriveManager } from "@/hooks/useDriveManager";
import { AutostartPage } from "@/pages/AutostartPage";
import { DrivesPage } from "@/pages/DrivesPage";
import { LogsPage } from "@/pages/LogsPage";
import { SettingsPage } from "@/pages/SettingsPage";
import type { ConnectionView, ViewName } from "@/types/drive";

export function App() {
  const manager = useDriveManager();
  const { showToast } = useToast();
  const reduceMotion = useReducedMotion();
  const [activeView, setActiveView] = useState<ViewName>("drives");
  const [connectionOpen, setConnectionOpen] = useState(false);
  const [editingProfile, setEditingProfile] = useState<ConnectionView | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ConnectionView | null>(null);
  const [forceExitOpen, setForceExitOpen] = useState(false);

  useEffect(() => {
    if (manager.exitError) setActiveView("drives");
  }, [manager.exitError]);

  const navigate = (view: ViewName) => {
    setActiveView(view);
    if (view === "logs") void manager.loadLogs();
    if (view === "autostart") void manager.refreshAutostart();
  };

  const openCreate = () => {
    setEditingProfile(null);
    setConnectionOpen(true);
  };

  const openEdit = (profile: ConnectionView) => {
    setEditingProfile(profile);
    setConnectionOpen(true);
  };

  const copyLogs = async () => {
    try {
      await navigator.clipboard.writeText(manager.logs.join("\n"));
      showToast("日志已复制");
    } catch (error) {
      showToast(`复制失败：${String(error)}`, "danger");
    }
  };

  const refreshAll = async () => {
    await manager.refresh(true);
    await manager.refreshAutostart();
    if (activeView === "logs") await manager.loadLogs();
  };

  const page = (() => {
    if (activeView === "autostart") {
      return (
        <AutostartPage
          status={manager.autostart}
          busy={manager.autostartBusy}
          error={manager.autostartError}
          onInstall={(mode) => void manager.installAutostart(mode)}
          onUninstall={() => void manager.uninstallAutostart()}
        />
      );
    }
    if (activeView === "logs") {
      return (
        <LogsPage
          lines={manager.logs}
          loading={manager.logsLoading}
          error={manager.logsError}
          onReload={() => void manager.loadLogs(true)}
          onCopy={() => void copyLogs()}
        />
      );
    }
    if (activeView === "settings") return <SettingsPage status={manager.status} />;
    return (
      <DrivesPage
        profiles={manager.profiles}
        mounts={manager.status.mounts}
        selectedId={manager.selectedId}
        operations={manager.operations}
        errors={manager.errors}
        exitError={manager.exitError}
        exitBusy={manager.exitBusy}
        onCreate={openCreate}
        onSelect={manager.setSelectedId}
        onAction={(id, action) => void manager.runAction(id, action)}
        onEdit={openEdit}
        onDelete={setDeleteTarget}
        onRetryExit={() => void manager.retryExit()}
        onForceExit={() => setForceExitOpen(true)}
      />
    );
  })();

  return (
    <>
      <AppLayout
        activeView={activeView}
        status={manager.status}
        refreshing={manager.refreshing}
        onNavigate={navigate}
        onRefresh={() => void refreshAll().catch(() => {})}
      >
        {manager.loadError ? (
          <div className="app-load-error" role="alert">
            无法刷新应用状态：{manager.loadError}
          </div>
        ) : null}
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={activeView}
            className="page-motion"
            initial={reduceMotion ? false : { opacity: 0, y: 2 }}
            animate={{ opacity: 1, y: 0 }}
            exit={reduceMotion ? undefined : { opacity: 0 }}
            transition={{ duration: reduceMotion ? 0 : 0.12, ease: "easeOut" }}
          >
            {page}
          </motion.div>
        </AnimatePresence>
      </AppLayout>

      <ConnectionDialog
        open={connectionOpen}
        profile={editingProfile}
        onOpenChange={setConnectionOpen}
        onSave={manager.saveConnection}
      />

      <ConfirmDialog
        open={Boolean(deleteTarget)}
        title="删除连接？"
        description={deleteTarget
          ? `将删除“${deleteTarget.name}”，并卸载驱动器、移除引擎侧配置。此操作无法撤销。`
          : ""}
        confirmLabel="删除"
        danger
        onOpenChange={(open) => { if (!open) setDeleteTarget(null); }}
        onConfirm={async () => {
          if (!deleteTarget) return;
          const deleted = await manager.deleteConnection(deleteTarget);
          if (deleted) setDeleteTarget(null);
        }}
      />

      <ConfirmDialog
        open={forceExitOpen}
        title="强制退出应用？"
        description="强制退出会跳过卸载确认，正在写入的文件可能受损。仅在重试退出仍失败时使用。"
        confirmLabel="仍然退出"
        danger
        busy={manager.exitBusy}
        onOpenChange={setForceExitOpen}
        onConfirm={() => manager.forceExit()}
      />
    </>
  );
}
