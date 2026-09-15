import { AlertDialog as AlertDialogPrimitive } from "@base-ui/react/alert-dialog";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  description: string;
  confirmLabel: string;
  danger?: boolean;
  busy?: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void | Promise<void>;
}

export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel,
  danger = false,
  busy = false,
  onOpenChange,
  onConfirm,
}: ConfirmDialogProps) {
  const [submitting, setSubmitting] = useState(false);
  const waiting = busy || submitting;

  useEffect(() => {
    if (!open) setSubmitting(false);
  }, [open]);

  return (
    <AlertDialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <AlertDialogPrimitive.Portal>
        <AlertDialogPrimitive.Backdrop className="dialog-backdrop" />
        <AlertDialogPrimitive.Viewport className="dialog-viewport">
          <AlertDialogPrimitive.Popup className="dialog-popup confirm-popup">
            <AlertDialogPrimitive.Title className="dialog-title">{title}</AlertDialogPrimitive.Title>
            <AlertDialogPrimitive.Description className="dialog-description">
              {description}
            </AlertDialogPrimitive.Description>
            <div className="dialog-actions">
              <AlertDialogPrimitive.Close className="button-default" disabled={waiting}>
                取消
              </AlertDialogPrimitive.Close>
              <Button
                variant={danger ? "danger" : "primary"}
                disabled={waiting}
                onClick={async () => {
                  setSubmitting(true);
                  try {
                    await onConfirm();
                  } finally {
                    setSubmitting(false);
                  }
                }}
              >
                {waiting ? "正在处理…" : confirmLabel}
              </Button>
            </div>
          </AlertDialogPrimitive.Popup>
        </AlertDialogPrimitive.Viewport>
      </AlertDialogPrimitive.Portal>
    </AlertDialogPrimitive.Root>
  );
}
