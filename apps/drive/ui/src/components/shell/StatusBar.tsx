import { FileText, RefreshCw } from "lucide-react";
import { StatusIndicator } from "@/components/feedback/StatusIndicator";
import { Button } from "@/components/ui/button";
import type { AppStatus, StatusTone } from "@/types/drive";

function statusText(status: AppStatus): { label: string; tone: StatusTone } {
  if (status.engine.running) {
    return {
      label: `rclone 引擎运行中${status.engine.version ? ` · ${status.engine.version}` : ""}`,
      tone: "success",
    };
  }
  if (status.engine.installed) return { label: "rclone 引擎未启动", tone: "neutral" };
  return { label: "未找到 rclone", tone: "danger" };
}

export function StatusBar({
  status,
  refreshing,
  onRefresh,
  onOpenLogs,
}: {
  status: AppStatus;
  refreshing: boolean;
  onRefresh: () => void;
  onOpenLogs: () => void;
}) {
  const engine = statusText(status);
  return (
    <footer className="statusbar">
      <StatusIndicator label={engine.label} tone={engine.tone} compact />
      <div className="statusbar-actions">
        <Button variant="subtle" size="compact" onClick={onRefresh} disabled={refreshing}>
          <RefreshCw className={refreshing ? "animate-spin" : undefined} aria-hidden="true" size={14} />
          {refreshing ? "刷新中" : "刷新"}
        </Button>
        <span className="statusbar-divider" aria-hidden="true" />
        <Button variant="subtle" size="compact" onClick={onOpenLogs}>
          <FileText aria-hidden="true" size={14} />
          查看日志
        </Button>
      </div>
    </footer>
  );
}
