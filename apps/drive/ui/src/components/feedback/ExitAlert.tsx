import { TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";

export function ExitAlert({
  message,
  busy,
  onRetry,
  onForce,
}: {
  message: string;
  busy: boolean;
  onRetry: () => void;
  onForce: () => void;
}) {
  return (
    <div className="exit-alert" role="alert">
      <TriangleAlert aria-hidden="true" size={18} />
      <div className="exit-alert-copy">
        <strong>部分虚拟硬盘无法卸载，软件尚未退出</strong>
        <span>{message || "请关闭正在使用虚拟硬盘的程序后重试。"}</span>
      </div>
      <div className="exit-alert-actions">
        <Button size="compact" disabled={busy} onClick={onRetry}>
          {busy ? "正在重试…" : "重试退出"}
        </Button>
        <Button size="compact" variant="danger" disabled={busy} onClick={onForce}>强制退出</Button>
      </div>
    </div>
  );
}
