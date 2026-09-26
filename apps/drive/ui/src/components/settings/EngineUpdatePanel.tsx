import { useCallback, useEffect, useState } from "react";
import { Download, FolderOpen, LoaderCircle, RefreshCw } from "lucide-react";
import { useToast } from "@/components/feedback/ToastProvider";
import { SettingsRow } from "@/components/settings/SettingsRow";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Input } from "@/components/ui/input";
import { driveApi } from "@/lib/tauri";
import { errorMessage } from "@/lib/utils";
import type { EngineStatus, EngineUpdateInfo } from "@/types/drive";

type Busy = "check" | "install" | "file" | "mirror" | null;
type PendingInstall =
  | { kind: "release"; version: string; label: string }
  | { kind: "file"; label: string };

function formatCheckTime(unixSeconds: number | null): string | null {
  if (!unixSeconds) return null;
  return new Date(unixSeconds * 1000).toLocaleString();
}

/**
 * 「引擎（rclone）」区块：当前版本、检查更新、可用版本/更新说明、更新源、本地安装。
 *
 * 检查失败静默降级：网络错误只在这里显示「上次检查失败/可重试」，不弹窗；
 * 安装是用户显式确认的动作，失败信息（卡在第几步、有没有回滚）必须完整显示。
 */
export function EngineUpdatePanel({ engine }: { engine: EngineStatus }) {
  const { showToast } = useToast();
  const [info, setInfo] = useState<EngineUpdateInfo | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState("");
  const [mirror, setMirror] = useState("");
  const [localPath, setLocalPath] = useState("");
  const [pending, setPending] = useState<PendingInstall | null>(null);

  const load = useCallback(async () => {
    try {
      const next = await driveApi.engineStatus();
      setInfo(next);
      setMirror(next.mirror_prefix);
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  // 只在这里取一次引擎更新状态：主界面 5 秒轮询不查引擎版本（避免反复起进程）。
  useEffect(() => {
    void load();
  }, [load]);

  const check = async () => {
    setBusy("check");
    setError("");
    try {
      const next = await driveApi.checkEngineUpdate(true);
      setInfo(next);
      if (next.last_error) {
        // 静默降级：失败只落在设置页上，不弹窗。
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

  const saveMirror = async () => {
    setBusy("mirror");
    setError("");
    try {
      const next = await driveApi.setEngineMirrorPrefix(mirror);
      setInfo(next);
      setMirror(next.mirror_prefix);
      showToast(next.mirror_prefix ? "已改用镜像前缀" : "已切回 rclone 官方源");
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  const browse = async () => {
    setError("");
    try {
      const picked = await driveApi.pickEngineFile();
      if (picked) setLocalPath(picked);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const runInstall = async () => {
    const target = pending;
    if (!target) return;
    setBusy(target.kind === "release" ? "install" : "file");
    setError("");
    try {
      const next =
        target.kind === "release"
          ? await driveApi.installEngineUpdate(target.version)
          : await driveApi.installEngineFromFile(localPath);
      setInfo(next);
      setPending(null);
      showToast("引擎已更新到 v" + (next.installed_version ?? "?"));
    } catch (err) {
      // 安装失败要说清"卡在第几步/是否已回滚"，所以放在区块内完整展示。
      setError(errorMessage(err));
      setPending(null);
      await load();
    } finally {
      setBusy(null);
    }
  };

  const version = info?.installed_version ?? engine.version ?? "-";
  const lastCheck = formatCheckTime(info?.last_check_at ?? null);
  const available = info?.latest_version ?? null;
  const hasUpdate = Boolean(info?.update_available && available);
  // 本机没有引擎（安装包没带上 / 被删 / 从零开始）时必须也能装：
  // update_available 在没有本地版本时恒为 false，只看它就会出现"没 rclone 也没法下载"。
  const engineMissing = !engine.installed;
  const canInstall = Boolean(available) && (hasUpdate || engineMissing);

  return (
    <>
      <SettingsRow label="当前版本" description={engine.path ?? "未找到 rclone.exe"}>
        <span className="setting-value">
          {engine.installed
            ? (engine.running ? "运行中 · v" : "未启动 · v") + version
            : "未安装"}
        </span>
      </SettingsRow>

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
        <Button variant="default" size="compact" disabled={busy !== null} onClick={() => void check()}>
          {busy === "check" ? (
            <LoaderCircle className="animate-spin" aria-hidden="true" size={14} />
          ) : (
            <RefreshCw aria-hidden="true" size={14} />
          )}
          {busy === "check" ? "正在检查…" : "检查更新"}
        </Button>
      </SettingsRow>

      <SettingsRow
        label="可用版本"
        description={
          engineMissing
            ? available
              ? "本机还没有 rclone 引擎：点「安装」会下载官方包并放到程序目录"
              : "本机还没有 rclone 引擎：请先点上面的「检查更新」获取可用版本"
            : hasUpdate
              ? "安装会先停止 rclone 并替换引擎文件，期间虚拟硬盘会短暂中断"
              : available
                ? "当前已是最新版本（rclone v" + available + "）"
                : "尚未检查过更新"
        }
      >
        <div className="engine-control">
          <span className="setting-value">{available ? "v" + available : "-"}</span>
          {canInstall ? (
            <Button
              variant="primary"
              size="compact"
              disabled={busy !== null}
              onClick={() =>
                setPending({
                  kind: "release",
                  version: available ?? "",
                  label: "安装 rclone v" + available,
                })
              }
            >
              {busy === "install" ? (
                <LoaderCircle className="animate-spin" aria-hidden="true" size={14} />
              ) : (
                <Download aria-hidden="true" size={14} />
              )}
              安装
            </Button>
          ) : null}
        </div>
      </SettingsRow>

      <SettingsRow
        label="更新源（镜像前缀）"
        description={"留空 = rclone 官方；填镜像前缀时按「前缀 + 官方地址」拼接。当前：" + (info?.source ?? "-")}
      >
        <div className="engine-control">
          <Input
            className="engine-input"
            aria-label="镜像前缀"
            placeholder="留空使用官方源，例如 https://ghfast.top"
            value={mirror}
            disabled={busy !== null}
            onChange={(event) => setMirror(event.target.value)}
          />
          <Button variant="default" size="compact" disabled={busy !== null} onClick={() => void saveMirror()}>
            {busy === "mirror" ? "保存中…" : "保存"}
          </Button>
        </div>
      </SettingsRow>

      <SettingsRow
        label="从本地文件安装"
        description="选择 rclone.exe 或官方 zip 包（用于离线/内网环境）"
      >
        <div className="engine-control">
          <Input
            className="engine-input"
            aria-label="本地引擎文件路径"
            placeholder="rclone.exe 或 rclone-vX.Y.Z-windows-amd64.zip"
            value={localPath}
            disabled={busy !== null}
            onChange={(event) => setLocalPath(event.target.value)}
          />
          <Button variant="default" size="compact" disabled={busy !== null} onClick={() => void browse()}>
            <FolderOpen aria-hidden="true" size={14} />
            浏览
          </Button>
          <Button
            variant="primary"
            size="compact"
            disabled={busy !== null || !localPath.trim()}
            onClick={() =>
              setPending({ kind: "file", label: "安装本地引擎文件" })
            }
          >
            安装
          </Button>
        </div>
      </SettingsRow>

      {error ? (
        <div className="settings-inline-error" role="alert">
          {error}
        </div>
      ) : null}

      {info?.notes ? (
        <details className="engine-notes">
          <summary>更新说明</summary>
          <pre>{info.notes}</pre>
        </details>
      ) : null}

      <ConfirmDialog
        open={Boolean(pending)}
        title="安装引擎更新？"
        description={
          "安装会中断所有挂载（当前必须已全部卸载）：先停止 rclone，再替换引擎文件，然后重启并校验版本与缓存选项。" +
          "校验失败会自动回滚到原引擎。\n\n" +
          (pending?.label ?? "")
        }
        confirmLabel="开始安装"
        busy={busy === "install" || busy === "file"}
        onOpenChange={(open) => {
          if (!open) setPending(null);
        }}
        onConfirm={runInstall}
      />
    </>
  );
}
