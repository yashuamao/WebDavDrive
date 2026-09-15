import { LoaderCircle } from "lucide-react";
import { StatusIndicator } from "@/components/feedback/StatusIndicator";
import { Button } from "@/components/ui/button";
import { DropdownMenu } from "@/components/ui/dropdown-menu";
import { cn, connectionStatus, displayDrive } from "@/lib/utils";
import type { ConnectionView, DriveAction, MountRecord, OperationState } from "@/types/drive";

interface DriveItemProps {
  profile: ConnectionView;
  mount: MountRecord | null;
  selected: boolean;
  operation?: OperationState;
  error?: string;
  onSelect: () => void;
  onAction: (action: DriveAction) => void;
  onEdit: () => void;
  onDelete: () => void;
}

export function DriveItem({
  profile,
  mount,
  selected,
  operation,
  error,
  onSelect,
  onAction,
  onEdit,
  onDelete,
}: DriveItemProps) {
  const status = connectionStatus(profile, mount, operation?.label, Boolean(error));
  const isBusy = Boolean(operation);
  const mainAction: DriveAction = mount ? "unmount" : "mount";
  const mainLabel = operation?.label ?? (mount ? "卸载" : error ? "重试" : "挂载");

  return (
    <article className={cn("drive-item", selected && "is-selected")}>
      <button
        type="button"
        className="drive-select"
        aria-pressed={selected}
        onClick={onSelect}
      >
        <span className="drive-letter" aria-label={`盘符 ${displayDrive(profile, mount)}`}>
          {displayDrive(profile, mount)}
        </span>
        <span className="drive-copy">
          <strong>{profile.name}</strong>
          <span title={profile.url}>{profile.url}</span>
        </span>
      </button>

      <div className="drive-meta">
        <StatusIndicator label={status.label} tone={status.tone} compact />
        <span className="drive-mode">{profile.read_only ? "只读" : "读写"}</span>
      </div>

      <div className="drive-actions">
        <Button
          size="compact"
          variant={mount ? "default" : "primary"}
          disabled={isBusy}
          onClick={() => onAction(mainAction)}
        >
          {isBusy ? <LoaderCircle className="animate-spin" aria-hidden="true" size={13} /> : null}
          {mainLabel}
        </Button>
        <DropdownMenu
          label={`${profile.name} 的更多操作`}
          actions={[
            ...(mount ? [{ label: "在资源管理器中打开", onSelect: () => onAction("open") }] : []),
            { label: "测试连接", onSelect: () => onAction("probe"), disabled: isBusy },
            { label: "编辑连接", onSelect: onEdit },
            { label: "删除连接", onSelect: onDelete, danger: true, separatorBefore: true },
          ]}
        />
      </div>

      {error ? (
        <div className="drive-inline-error" role="alert">
          <span>{error}</span>
          <button type="button" onClick={() => onAction(mainAction)}>重试</button>
        </div>
      ) : null}
    </article>
  );
}
