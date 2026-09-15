import { Plus } from "lucide-react";
import { DriveDetail } from "@/components/drive/DriveDetail";
import { DriveList } from "@/components/drive/DriveList";
import { ExitAlert } from "@/components/feedback/ExitAlert";
import { Button } from "@/components/ui/button";
import type { ConnectionView, DriveAction, MountRecord, OperationState } from "@/types/drive";

interface DrivesPageProps {
  profiles: ConnectionView[];
  mounts: MountRecord[];
  selectedId: string | null;
  operations: Record<string, OperationState | undefined>;
  errors: Record<string, string | undefined>;
  exitError: string;
  exitBusy: boolean;
  onCreate: () => void;
  onSelect: (id: string) => void;
  onAction: (id: string, action: DriveAction) => void;
  onEdit: (profile: ConnectionView) => void;
  onDelete: (profile: ConnectionView) => void;
  onRetryExit: () => void;
  onForceExit: () => void;
}

export function DrivesPage(props: DrivesPageProps) {
  const selected = props.profiles.find((profile) => profile.id === props.selectedId) ?? null;

  return (
    <section className="page drives-page" aria-labelledby="drives-title">
      <header className="page-header">
        <div>
          <div className="title-row">
            <h1 id="drives-title">我的驱动器</h1>
            <span className="count-label">{props.profiles.length} 个连接</span>
          </div>
          <p>管理 WebDAV 连接与本地挂载状态</p>
        </div>
        <Button variant="primary" onClick={props.onCreate}>
          <Plus aria-hidden="true" size={15} />
          新建连接
        </Button>
      </header>

      {props.exitError ? (
        <ExitAlert
          message={props.exitError}
          busy={props.exitBusy}
          onRetry={props.onRetryExit}
          onForce={props.onForceExit}
        />
      ) : null}

      <div className="drive-workspace">
        <section className="drive-list-pane">
          <DriveList {...props} />
        </section>
        <DriveDetail
          profile={selected}
          mounts={props.mounts}
          operation={selected ? props.operations[selected.id] : undefined}
          error={selected ? props.errors[selected.id] : undefined}
          onAction={(action) => selected && props.onAction(selected.id, action)}
          onEdit={() => selected && props.onEdit(selected)}
        />
      </div>
    </section>
  );
}
