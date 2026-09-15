import { AnimatePresence, motion } from "motion/react";
import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";
import { CircleAlert, CircleCheck, X } from "lucide-react";

type ToastTone = "success" | "danger";

interface ToastItem {
  id: number;
  message: string;
  tone: ToastTone;
}

interface ToastContextValue {
  showToast: (message: string, tone?: ToastTone) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toast, setToast] = useState<ToastItem | null>(null);
  const timer = useRef<number | null>(null);

  const dismiss = useCallback(() => setToast(null), []);
  const showToast = useCallback((message: string, tone: ToastTone = "success") => {
    if (timer.current) window.clearTimeout(timer.current);
    setToast({ id: Date.now(), message, tone });
    timer.current = window.setTimeout(dismiss, tone === "danger" ? 6000 : 3000);
  }, [dismiss]);

  const value = useMemo(() => ({ showToast }), [showToast]);

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div className="toast-region" aria-live="polite" aria-atomic="true">
        <AnimatePresence>
          {toast ? (
            <motion.div
              key={toast.id}
              className="toast"
              data-tone={toast.tone}
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 4 }}
              transition={{ duration: 0.15, ease: "easeOut" }}
              role="status"
            >
              {toast.tone === "danger"
                ? <CircleAlert aria-hidden="true" size={16} />
                : <CircleCheck aria-hidden="true" size={16} />}
              <span>{toast.message}</span>
              <button className="toast-close" type="button" aria-label="关闭通知" onClick={dismiss}>
                <X aria-hidden="true" size={14} />
              </button>
            </motion.div>
          ) : null}
        </AnimatePresence>
      </div>
    </ToastContext.Provider>
  );
}

export function useToast() {
  const value = useContext(ToastContext);
  if (!value) throw new Error("useToast 必须在 ToastProvider 内使用");
  return value;
}
