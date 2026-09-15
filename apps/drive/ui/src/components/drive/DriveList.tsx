import { EmptyState } from "@/components/feedback/EmptyState";
import { DriveItem } from "@/components/drive/DriveItem";
import { mountFor } from "@/lib/utils";
import type { ConnectionView, DriveAction, MountRecord, OperationState } from "@/types/drive";

interface DriveListProps {
  profiles: ConnectionView[];
  mounts: MountRecord[];
  selectedId: string | null;
  operations: Record<string, OperationState | undefined>;
  errors: Record<string, string | undefined>;
  onCreate: () => void;
  onSelect: (id: string) => void;
  onAction: (id: string, action: DriveAction) => void;
  onEdit: (profile: ConnectionView) => void;
  onDelete: (profile: ConnectionView) => void;
}

export function DriveList(props: DriveListProps) {
  if (!props.profiles.length) return <EmptyState onCreate={props.onCreate} />;

  return (
    <div className="drive-list" role="list" aria-label="已保存的 WebDAV 连接">
      {props.profiles.map((profile) => (
        <div role="listitem" key={profile.id}>
          <DriveItem
            profile={profile}
            mount={mountFor(profile, props.mounts)}
            selected={profile.id === props.selectedId}
            operation={props.operations[profile.id]}
            error={props.errors[profile.id]}
            onSelect={() => props.onSelect(profile.id)}
            onAction={(action) => props.onAction(profile.id, action)}
            onEdit={() => props.onEdit(profile)}
            onDelete={() => props.onDelete(profile)}
          />
        </div>
      ))}
    </div>
  );
}
