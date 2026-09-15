import { Copy, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";

export function LogsPage({
  lines,
  loading,
  error,
  onReload,
  onCopy,
}: {
  lines: string[];
  loading: boolean;
  error: string;
  onReload: () => void;
  onCopy: () => void;
}) {
  return (
    <section className="page logs-page" aria-labelledby="logs-title">
      <header className="page-header">
        <div>
          <h1 id="logs-title">运行日志</h1>
          <p>查看最近的引擎与挂载事件</p>
        </div>
        <div className="page-actions">
          <Button onClick={onCopy} disabled={!lines.length}>
            <Copy aria-hidden="true" size={14} />
            复制
          </Button>
          <Button onClick={onReload} disabled={loading}>
            <RefreshCw className={loading ? "animate-spin" : undefined} aria-hidden="true" size={14} />
            {loading ? "刷新中" : "刷新"}
          </Button>
        </div>
      </header>
      {error ? <div className="settings-inline-error" role="alert">{error}</div> : null}
      <section className="log-viewer" aria-label="应用日志">
        <pre tabIndex={0}>{lines.length ? lines.join("\n") : loading ? "正在读取日志…" : "暂无日志"}</pre>
      </section>
    </section>
  );
}
