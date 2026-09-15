import { HardDrive, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";

export function EmptyState({ onCreate }: { onCreate: () => void }) {
  return (
    <div className="empty-state">
      <div className="empty-icon" aria-hidden="true"><HardDrive size={22} /></div>
      <strong>还没有 WebDAV 连接</strong>
      <p>添加连接后即可挂载到 Windows 资源管理器。</p>
      <Button variant="primary" onClick={onCreate}>
        <Plus aria-hidden="true" size={15} />
        新建连接
      </Button>
    </div>
  );
}
