import { useEffect, useRef, useState, type FormEvent } from "react";
import { LoaderCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogCloseButton,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, type SelectOption } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { errorMessage } from "@/lib/utils";
import type { ConnectionInput, ConnectionView } from "@/types/drive";

const vendorOptions: SelectOption[] = [
  { value: "other", label: "通用 WebDAV" },
  { value: "nextcloud", label: "Nextcloud" },
  { value: "owncloud", label: "ownCloud" },
  { value: "sharepoint", label: "SharePoint" },
  { value: "rclone", label: "rclone WebDAV" },
];

const vfsOptions: SelectOption[] = [
  { value: "off", label: "关闭（Office 文件可能受限）" },
  { value: "minimal", label: "最小缓存" },
  { value: "writes", label: "写入缓存（推荐）" },
  { value: "full", label: "完整缓存" },
];

function formValue(profile: ConnectionView | null): ConnectionInput {
  return {
    id: profile?.id ?? null,
    name: profile?.name ?? "",
    url: profile?.url ?? "",
    vendor: profile?.vendor ?? "other",
    user: profile?.user ?? "",
    password: "",
    clear_password: false,
    drive: profile?.drive ?? "X:",
    volname: profile?.volname ?? "",
    network_mode: profile?.network_mode ?? false,
    vfs_cache_mode: profile?.vfs_cache_mode ?? "writes",
    dir_cache_time: profile?.dir_cache_time ?? "5m",
    read_only: profile?.read_only ?? false,
    autostart: profile?.autostart ?? true,
    extra_opts: profile?.extra_opts ?? "",
  };
}

export function ConnectionDialog({
  open,
  profile,
  onOpenChange,
  onSave,
}: {
  open: boolean;
  profile: ConnectionView | null;
  onOpenChange: (open: boolean) => void;
  onSave: (input: ConnectionInput) => Promise<void>;
}) {
  const [form, setForm] = useState<ConnectionInput>(() => formValue(profile));
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const errorRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open) {
      setForm(formValue(profile));
      setError("");
    }
  }, [open, profile]);

  const update = <K extends keyof ConnectionInput>(key: K, value: ConnectionInput[K]) => {
    setForm((current) => ({ ...current, [key]: value }));
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const normalized = {
      ...form,
      name: form.name.trim(),
      url: form.url.trim(),
      dir_cache_time: form.dir_cache_time.trim() || "5m",
      extra_opts: form.extra_opts.trim(),
    };
    if (!normalized.name || !normalized.url) {
      setError("请填写连接名称和 WebDAV 地址。");
      requestAnimationFrame(() => errorRef.current?.focus());
      return;
    }
    setSaving(true);
    setError("");
    try {
      await onSave(normalized);
      onOpenChange(false);
    } catch (cause) {
      setError(errorMessage(cause));
      requestAnimationFrame(() => errorRef.current?.focus());
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="connection-dialog">
        <form onSubmit={(event) => void submit(event)} autoComplete="on">
          <header className="dialog-header">
            <div>
              <DialogTitle className="dialog-title">
                {profile ? `编辑 ${profile.name}` : "新建连接"}
              </DialogTitle>
              <DialogDescription className="dialog-description">
                配置 WebDAV 服务与本地挂载选项
              </DialogDescription>
            </div>
            <DialogCloseButton />
          </header>

          <div className="dialog-body">
            {error ? (
              <div ref={errorRef} className="form-error-summary" role="alert" tabIndex={-1}>
                <strong>无法保存连接</strong>
                <span>{error}</span>
              </div>
            ) : null}

            <section className="form-section" aria-labelledby="connection-fields-title">
              <h3 id="connection-fields-title">连接信息</h3>
              <div className="form-grid">
                <label className="field">
                  <span>名称</span>
                  <Input
                    autoFocus
                    required
                    value={form.name}
                    placeholder="我的 NAS"
                    onChange={(event) => update("name", event.target.value)}
                  />
                </label>
                <label className="field">
                  <span>服务端类型</span>
                  <Select
                    label="服务端类型"
                    value={form.vendor}
                    options={vendorOptions}
                    onValueChange={(value) => update("vendor", value)}
                  />
                </label>
                <label className="field full">
                  <span>WebDAV 地址</span>
                  <Input
                    required
                    type="url"
                    value={form.url}
                    placeholder="https://nas.example.com/dav"
                    onChange={(event) => update("url", event.target.value)}
                  />
                </label>
                <label className="field">
                  <span>用户名</span>
                  <Input
                    autoComplete="username"
                    value={form.user}
                    placeholder="alice"
                    onChange={(event) => update("user", event.target.value)}
                  />
                </label>
                <label className="field">
                  <span>密码</span>
                  <Input
                    type="password"
                    autoComplete="current-password"
                    disabled={form.clear_password}
                    value={form.password}
                    placeholder={profile?.password_set ? "留空表示不修改" : "可留空"}
                    onChange={(event) => {
                      update("password", event.target.value);
                      if (event.target.value) update("clear_password", false);
                    }}
                  />
                </label>
              </div>
              <label className="checkbox-row" data-disabled={!profile?.password_set || undefined}>
                <input
                  type="checkbox"
                  disabled={!profile?.password_set}
                  checked={form.clear_password}
                  onChange={(event) => {
                    update("clear_password", event.target.checked);
                    if (event.target.checked) update("password", "");
                  }}
                />
                <span>清除已保存密码</span>
              </label>
            </section>

            <section className="form-section" aria-labelledby="mount-fields-title">
              <h3 id="mount-fields-title">挂载选项</h3>
              <div className="form-grid">
                <label className="field">
                  <span>盘符或目录</span>
                  <Input value={form.drive} placeholder="X: 或 C:\\mnt\\nas" onChange={(event) => update("drive", event.target.value)} />
                </label>
                <label className="field">
                  <span>卷标</span>
                  <Input value={form.volname} placeholder="留空则使用名称" onChange={(event) => update("volname", event.target.value)} />
                </label>
                <label className="field">
                  <span>VFS 缓存模式</span>
                  <Select
                    label="VFS 缓存模式"
                    value={form.vfs_cache_mode}
                    options={vfsOptions}
                    onValueChange={(value) => update("vfs_cache_mode", value)}
                  />
                </label>
                <label className="field">
                  <span>目录缓存时间</span>
                  <Input value={form.dir_cache_time} onChange={(event) => update("dir_cache_time", event.target.value)} />
                </label>
              </div>

              <div className="toggle-grid">
                <label className="toggle-field">
                  <span className="toggle-field-copy"><strong>网络驱动器</strong><small>在资源管理器中显示为网络位置</small></span>
                  <Switch label="网络驱动器" checked={form.network_mode} onCheckedChange={(checked) => update("network_mode", checked)} />
                </label>
                <label className="toggle-field">
                  <span className="toggle-field-copy"><strong>只读</strong><small>阻止远端文件被修改</small></span>
                  <Switch label="只读" checked={form.read_only} onCheckedChange={(checked) => update("read_only", checked)} />
                </label>
                <label className="toggle-field">
                  <span className="toggle-field-copy"><strong>启动时自动挂载</strong><small>应用启动后恢复此连接</small></span>
                  <Switch label="启动时自动挂载" checked={form.autostart} onCheckedChange={(checked) => update("autostart", checked)} />
                </label>
              </div>

              <label className="field">
                <span>附加 rclone 参数</span>
                <Input
                  value={form.extra_opts}
                  placeholder="--vfs-cache-max-size 10G"
                  spellCheck={false}
                  onChange={(event) => update("extra_opts", event.target.value)}
                />
              </label>
            </section>
          </div>

          <footer className="dialog-footer">
            <Button disabled={saving} onClick={() => onOpenChange(false)}>取消</Button>
            <Button type="submit" variant="primary" disabled={saving}>
              {saving ? <LoaderCircle className="animate-spin" aria-hidden="true" size={14} /> : null}
              {saving ? "正在保存…" : "保存连接"}
            </Button>
          </footer>
        </form>
      </DialogContent>
    </Dialog>
  );
}
