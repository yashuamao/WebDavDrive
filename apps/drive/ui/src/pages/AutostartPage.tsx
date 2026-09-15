import { useEffect, useState } from "react";
import { LoaderCircle } from "lucide-react";
import { StatusIndicator } from "@/components/feedback/StatusIndicator";
import { SettingsGroup } from "@/components/settings/SettingsGroup";
import { SettingsRow } from "@/components/settings/SettingsRow";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import type { AutostartStatus } from "@/types/drive";

interface AutostartPageProps {
  status: AutostartStatus | null;
  busy: boolean;
  error: string;
  onInstall: (mode: "logon" | "boot") => void;
  onUninstall: () => void;
}

export function AutostartPage({ status, busy, error, onInstall, onUninstall }: AutostartPageProps) {
  const [mode, setMode] = useState<"logon" | "boot">("logon");
  useEffect(() => {
    if (status?.mode === "boot" || status?.mode === "logon") setMode(status.mode);
  }, [status]);

  return (
    <section className="page narrow-page" aria-labelledby="autostart-title">
      <header className="page-header">
        <div>
          <h1 id="autostart-title">自动挂载</h1>
          <p>选择应用在登录或系统启动时的运行方式</p>
        </div>
      </header>

      <SettingsGroup title="启动任务" description="注册和移除任务可能需要管理员权限">
        <SettingsRow label="当前状态" description={status?.task_name ? `任务：${status.task_name}` : undefined}>
          <StatusIndicator
            label={status?.installed ? "已注册" : "未注册"}
            tone={status?.installed ? "success" : "neutral"}
          />
        </SettingsRow>
        <SettingsRow
          label="启动模式"
          description="登录模式适合当前用户；开机模式由 SYSTEM 运行并提供全局盘符。"
        >
          <Select
            className="w-[250px]"
            label="自动挂载启动模式"
            value={mode}
            disabled={busy}
            options={[
              { value: "logon", label: "登录时（当前用户，推荐）" },
              { value: "boot", label: "开机时（SYSTEM，全局盘符）" },
            ]}
            onValueChange={(value) => setMode(value as "logon" | "boot")}
          />
        </SettingsRow>
        {error ? <div className="settings-inline-error" role="alert">{error}</div> : null}
        <div className="settings-actions">
          <Button variant="primary" disabled={busy} onClick={() => onInstall(mode)}>
            {busy ? <LoaderCircle className="animate-spin" aria-hidden="true" size={14} /> : null}
            {busy ? "正在处理…" : status?.installed ? "更新任务" : "注册任务"}
          </Button>
          <Button variant="ghostDanger" disabled={busy || !status?.installed} onClick={onUninstall}>
            移除任务
          </Button>
        </div>
      </SettingsGroup>
    </section>
  );
}
