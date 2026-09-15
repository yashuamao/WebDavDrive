import { FolderOpen, Pencil } from "lucide-react";
import { StatusIndicator } from "@/components/feedback/StatusIndicator";
import { Button } from "@/components/ui/button";
import { connectionStatus, displayDrive, mountFor } from "@/lib/utils";
import type { ConnectionView, DriveAction, MountRecord, OperationState } from "@/types/drive";

const vfsLabels: Record<string, string> = {
  off: "关闭",
  minimal: "最小",
  writes: "写入",
  full: "完整",
};

interface DriveDetailProps {
  profile: ConnectionView | null;
  mounts: MountRecord[];
  operation?: OperationState;
  error?: string;
  onAction: (action: DriveAction) => void;
  onEdit: () => void;
}

export function DriveDetail({ profile, mounts, operation, error, onAction, onEdit }: DriveDetailProps) {
  if (!profile) {
    return <aside className="detail-pane detail-empty">选择一个连接查看详情</aside>;
  }
  const mount = mountFor(profile, mounts);
  const status = connectionStatus(profile, mount, operation?.label, Boolean(error));
  const point = mount?.mount_point || profile.drive || "-";

  return (
    <aside className="detail-pane" aria-label={`${profile.name} 连接详情`}>
      <div className="detail-identity">
        <span className="drive-letter large" aria-hidden="true">{displayDrive(profile, mount)}</span>
        <div className="min-w-0">
          <h2>{profile.name}</h2>
          <p title={profile.url}>{profile.url}</p>
          <StatusIndicator label={status.label} tone={status.tone} compact />
        </div>
      </div>

      <section className="detail-section">
        <h3>连接信息</h3>
        <dl className="detail-list">
          <div><dt>服务类型</dt><dd>{profile.vendor || "other"}</dd></div>
          <div><dt>账号</dt><dd>{profile.user || "匿名"}</dd></div>
          <div><dt>盘符</dt><dd>{point}</dd></div>
          <div><dt>网络驱动器</dt><dd>{profile.network_mode ? "是" : "否"}</dd></div>
        </dl>
      </section>

      <section className="detail-section">
        <h3>挂载选项</h3>
        <dl className="detail-list">
          <div><dt>VFS 缓存</dt><dd>{vfsLabels[profile.vfs_cache_mode] ?? profile.vfs_cache_mode}</dd></div>
          <div><dt>目录缓存</dt><dd>{profile.dir_cache_time}</dd></div>
          <div><dt>自动挂载</dt><dd>{profile.autostart ? "已启用" : "未启用"}</dd></div>
          <div><dt>访问模式</dt><dd>{profile.read_only ? "只读" : "读写"}</dd></div>
        </dl>
      </section>

      {error ? <div className="detail-error" role="alert">{error}</div> : null}

      <div className="detail-actions">
        {mount ? (
          <>
            <Button variant="primary" onClick={() => onAction("open")} disabled={Boolean(operation)}>
              <FolderOpen aria-hidden="true" size={15} />
              在资源管理器中打开
            </Button>
            <Button onClick={() => onAction("unmount")} disabled={Boolean(operation)}>卸载</Button>
          </>
        ) : (
          <Button variant="primary" onClick={() => onAction("mount")} disabled={Boolean(operation)}>
            {operation?.label ?? (error ? "重试挂载" : "挂载")}
          </Button>
        )}
        <Button onClick={() => onAction("probe")} disabled={Boolean(operation)}>测试连接</Button>
        <Button variant="subtle" onClick={onEdit}>
          <Pencil aria-hidden="true" size={14} />
          编辑配置
        </Button>
      </div>
    </aside>
  );
}
