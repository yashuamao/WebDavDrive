import { useCallback, useEffect, useState } from "react";
import { Download, LoaderCircle, RefreshCw } from "lucide-react";
import { useToast } from "@/components/feedback/ToastProvider";
import { SettingsRow } from "@/components/settings/SettingsRow";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { driveApi } from "@/lib/tauri";
import { errorMessage } from "@/lib/utils";
import type { AppUpdateInfo } from "@/types/drive";

type Busy = "check" | "download" | "install" | null;

/** 绿色版（免安装 zip）没有 uninstall.exe：自动更新会被拒绝，只能手动下载。 */
const RELEASES_URL = "https://github.com/yashuamao/WebDavDrive/releases/latest";

function formatCheckTime(unixSeconds: number | null): string | null {
  if (!unixSeconds) return null;
  return new Date(unixSeconds * 1000).toLocaleString();
}

/**
 * 「应用更新」区块：检查新版本 → 下载并校验安装包 → 静默安装并自动重启。
 *
 * 与引擎更新的区别：安装应用更新必须替换正在运行的 drive.exe，所以安装动作是
 * 「拉起更新程序 → 本应用优雅退出（卸挂载 + 停引擎）→ 更新程序静默安装 → 重启」。
 * 因此点下安装后界面只需要说明「正在退出安装」，不该再等 Promise 返回。
 *
 * 绿色版（免安装）不支持自动更新：这里只给出 GitHub Releases 地址。
 */
export function AppUpdatePanel() {
  const { showToast } = useToast();
  const [info, setInfo] = useState<AppUpdateInfo | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [exiting, setExiting] = useState(false);

  const load = useCallback(async () => {
    try {
      setInfo(await driveApi.appUpdateStatus());
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const check = async () => {
    setBusy("check");
    setError("");
    try {
      const next = await driveApi.checkAppUpdate(true);
      setInfo(next);
      if (next.last_error) {
        // 静默降级：网络失败只落在设置页上，不影响已挂载的盘。
        setError(next.last_error);
      } else if (next.update_available && next.latest_version) {
        showToast("发现新版本 v" + next.latest_version);
      } else {
        showToast("已是最新版本");
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  const download = async (version: string) => {
    setBusy("download");
    setError("");
    try {
      const next = await driveApi.downloadAppUpdate(version);
      setInfo(next);
      showToast("安装包已下载并校验通过");
    } catch (err) {
      setError(errorMessage(err));
      await load();
    } finally {
      setBusy(null);
    }
  };

  const install = async () => {
    setConfirming(false);
    setBusy("install");
    setError("");
    try {
      await driveApi.installAppUpdate();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(null);
      return;
    }
    // 命令成功后应用就开始退出了；下面的提示会一直留到窗口消失。
    setExiting(true);
  };

  const portable = Boolean(info?.portable);
  const available = info?.latest_version ?? null;
  const hasUpdate = Boolean(info?.update_available && available);
  const downloaded = Boolean(info?.setup_downloaded && available);
  const lastCheck = formatCheckTime(info?.last_check_at ?? null);

  const availableDescription = portable
    ? "绿色版不支持自动更新，请复制上面的地址到浏览器下载"
    : downloaded
      ? "安装包已下载并校验通过：点「安装并重启」会先卸下全部虚拟硬盘，再静默安装"
      : hasUpdate
        ? "安装会先卸载全部虚拟硬盘并停止 rclone，装好后自动重启应用"
        : available
          ? "当前已是最新版本（v" + available + "）"
          : "尚未检查过更新";

  return (
    <>
      <SettingsRow
        label="当前版本"
        description={
          portable
            ? "绿色版（免安装）：请到 GitHub Releases 下载新版本"
            : "安装版：可以自动下载并静默安装"
        }
      >
        <span className="setting-value">{info ? "v" + info.installed_version : "-"}</span>
      </SettingsRow>

      {portable ? (
        <SettingsRow
          label="手动更新"
          description="复制地址到浏览器下载新的 zip（覆盖程序目录即可，配置不受影响）"
        >
          <span className="setting-value">{RELEASES_URL}</span>
        </SettingsRow>
      ) : null}

      <SettingsRow
        label="检查更新"
        description={
          info?.last_error
            ? "上次检查失败，可重试（网络问题不影响已挂载的盘）"
            : lastCheck
              ? "上次检查：" + lastCheck
              : "自动检查每天最多一次；手动检查随时可用"
        }
      >
        <Button
          variant="default"
          size="compact"
          disabled={busy !== null || exiting}
          onClick={() => void check()}
        >
          {busy === "check" ? (
            <LoaderCircle className="animate-spin" aria-hidden="true" size={14} />
          ) : (
            <RefreshCw aria-hidden="true" size={14} />
          )}
          {busy === "check" ? "正在检查…" : "检查更新"}
        </Button>
      </SettingsRow>

      <SettingsRow label="可用版本" description={availableDescription}>
        <div className="engine-control">
          <span className="setting-value">{available ? "v" + available : "-"}</span>
          {hasUpdate && !downloaded ? (
            <Button
              variant="default"
              size="compact"
              disabled={busy !== null || exiting}
              onClick={() => void download(available ?? "")}
            >
              {busy === "download" ? (
                <LoaderCircle className="animate-spin" aria-hidden="true" size={14} />
              ) : (
                <Download aria-hidden="true" size={14} />
              )}
              {busy === "download" ? "正在下载…" : "下载更新"}
            </Button>
          ) : null}
          {downloaded ? (
            <Button
              variant="primary"
              size="compact"
              disabled={busy !== null || exiting}
              onClick={() => setConfirming(true)}
            >
              {exiting ? (
                <LoaderCircle className="animate-spin" aria-hidden="true" size={14} />
              ) : (
                <Download aria-hidden="true" size={14} />
              )}
              {exiting ? "正在安装…" : "安装并重启"}
            </Button>
          ) : null}
        </div>
      </SettingsRow>

      {error ? (
        <div className="settings-inline-error" role="alert">
          {error}
        </div>
      ) : null}

      {exiting ? (
        <div className="settings-inline-error" role="status">
          正在卸载全部挂载并关闭应用，更新程序会自动安装并重启（请不要手动结束进程）。
        </div>
      ) : null}

      {info?.notes ? (
        <details className="engine-notes">
          <summary>更新说明</summary>
          <pre>{info.notes}</pre>
        </details>
      ) : null}

      <ConfirmDialog
        open={confirming}
        title="安装应用更新？"
        description={
          "安装会立刻关闭应用：先卸载全部虚拟硬盘、停止 rclone，然后由更新程序静默安装到当前目录，" +
          "装好后自动重启应用（配置和连接都不会变）。\n\n" +
          (available ? "目标版本：v" + available : "")
        }
        confirmLabel="立即安装"
        busy={busy === "install"}
        onOpenChange={(open) => {
          if (!open) setConfirming(false);
        }}
        onConfirm={install}
      />
    </>
  );
}
