import { Clock3, FileText, HardDrive, Settings } from "lucide-react";
import { StatusIndicator } from "@/components/feedback/StatusIndicator";
import { cn } from "@/lib/utils";
import type { AppStatus, StatusTone, ViewName } from "@/types/drive";

const navigation = [
  { id: "drives", label: "驱动器", icon: HardDrive },
  { id: "autostart", label: "自动挂载", icon: Clock3 },
  { id: "logs", label: "日志", icon: FileText },
  { id: "settings", label: "设置", icon: Settings },
] satisfies { id: ViewName; label: string; icon: typeof HardDrive }[];

function engineSummary(status: AppStatus): { label: string; tone: StatusTone } {
  if (status.engine.running) return { label: "引擎运行中", tone: "success" };
  if (status.engine.installed) return { label: "引擎未启动", tone: "neutral" };
  return { label: "缺少 rclone", tone: "danger" };
}

export function Sidebar({
  activeView,
  status,
  onNavigate,
}: {
  activeView: ViewName;
  status: AppStatus;
  onNavigate: (view: ViewName) => void;
}) {
  const engine = engineSummary(status);
  return (
    <aside className="sidebar" aria-label="主导航">
      <div className="brand">
        <span className="brand-mark" aria-hidden="true"><HardDrive size={17} /></span>
        <span>WebDAV Drive</span>
      </div>
      <nav className="nav-list">
        {navigation.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            className={cn("nav-item", activeView === id && "is-active")}
            aria-current={activeView === id ? "page" : undefined}
            onClick={() => onNavigate(id)}
          >
            <Icon aria-hidden="true" size={17} />
            <span>{label}</span>
          </button>
        ))}
      </nav>
      <div className="sidebar-spacer" />
      <div className="sidebar-engine">
        <StatusIndicator label={engine.label} tone={engine.tone} compact />
      </div>
    </aside>
  );
}
