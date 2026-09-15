import { Sidebar } from "@/components/shell/Sidebar";
import { StatusBar } from "@/components/shell/StatusBar";
import type { AppStatus, ViewName } from "@/types/drive";

interface AppLayoutProps {
  activeView: ViewName;
  status: AppStatus;
  refreshing: boolean;
  children: React.ReactNode;
  onNavigate: (view: ViewName) => void;
  onRefresh: () => void;
}

export function AppLayout({
  activeView,
  status,
  refreshing,
  children,
  onNavigate,
  onRefresh,
}: AppLayoutProps) {
  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">跳到主要内容</a>
      <Sidebar activeView={activeView} status={status} onNavigate={onNavigate} />
      <section className="workspace">
        <main className="page-stack" id="main-content" tabIndex={-1}>{children}</main>
        <StatusBar
          status={status}
          refreshing={refreshing}
          onRefresh={onRefresh}
          onOpenLogs={() => onNavigate("logs")}
        />
      </section>
    </div>
  );
}
