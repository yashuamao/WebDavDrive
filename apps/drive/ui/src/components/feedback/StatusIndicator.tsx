import { LoaderCircle } from "lucide-react";
import { cn } from "@/lib/utils";
import type { StatusTone } from "@/types/drive";

export function StatusIndicator({
  label,
  tone,
  compact = false,
}: {
  label: string;
  tone: StatusTone;
  compact?: boolean;
}) {
  return (
    <span className={cn("status-indicator", compact && "text-xs")} data-tone={tone}>
      {tone === "busy"
        ? <LoaderCircle className="animate-spin" aria-hidden="true" size={compact ? 12 : 13} />
        : <span className="status-dot" aria-hidden="true" />}
      <span>{label}</span>
    </span>
  );
}
