import { Check } from "lucide-react";
import { StatusIndicator } from "@/components/feedback/StatusIndicator";
import { EngineUpdatePanel } from "@/components/settings/EngineUpdatePanel";
import { SettingsGroup } from "@/components/settings/SettingsGroup";
import { SettingsRow } from "@/components/settings/SettingsRow";
import { Select } from "@/components/ui/select";
import { useTheme } from "@/hooks/useTheme";
import type { AppStatus, ThemeMode } from "@/types/drive";

export function SettingsPage({ status }: { status: AppStatus }) {
  const { theme, setTheme } = useTheme();
  const engineTone = status.engine.running ? "success" : status.engine.installed ? "neutral" : "danger";

  return (
    <section className="page narrow-page" aria-labelledby="settings-title">
      <header className="page-header">
        <div>
          <h1 id="settings-title">设置</h1>
          <p>应用外观、运行环境与退出行为</p>
        </div>
      </header>

      <SettingsGroup title="外观">
        <SettingsRow label="应用主题" description="跟随 Windows 设置，或固定使用浅色/深色外观。">
          <Select
            className="w-[180px]"
            label="应用主题"
            value={theme}
            options={[
              { value: "system", label: "跟随系统" },
              { value: "light", label: "浅色" },
              { value: "dark", label: "深色" },
            ]}
            onValueChange={(value) => setTheme(value as ThemeMode)}
          />
        </SettingsRow>
      </SettingsGroup>

      <SettingsGroup title="运行环境" description="应用自动检测 rclone 与本机凭据保护">
        <SettingsRow label="rclone 引擎" description={status.engine.path ?? "未找到可用的 rclone.exe"}>
          <StatusIndicator
            label={status.engine.running ? "运行中" : status.engine.installed ? "未启动" : "未安装"}
            tone={engineTone}
          />
        </SettingsRow>
        <SettingsRow label="引擎版本">
          <span className="setting-value">{status.engine.version ?? "-"}</span>
        </SettingsRow>
        <SettingsRow label="凭据保护">
          <span className="setting-value">{status.secrets || "-"}</span>
        </SettingsRow>
        {!status.engine.installed ? (
          <div className="settings-inline-error" role="status">
            把 rclone.exe 放到程序同目录或 bin 子目录，或设置 RCLONE_EXE 环境变量。
          </div>
        ) : null}
      </SettingsGroup>

      <SettingsGroup
        title="引擎（rclone）"
        description="引擎独立更新：只提示、确认后才安装；安装前必须没有挂载，失败会自动回滚"
      >
        <EngineUpdatePanel engine={status.engine} />
      </SettingsGroup>

      <SettingsGroup title="退出行为">
        <SettingsRow
          label="退出前自动卸载全部驱动器"
          description="关闭窗口只隐藏到托盘；从托盘退出时，应用会确认全部盘符消失后再关闭。"
        >
          <span className="fixed-setting"><Check aria-hidden="true" size={14} />始终执行</span>
        </SettingsRow>
      </SettingsGroup>
    </section>
  );
}
