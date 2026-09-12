/* WebDAV Drive —— Tauri 前端（无构建，直接使用 window.__TAURI__ 注入的 IPC）。 */
'use strict';

const $ = (id) => document.getElementById(id);
const invoke = (cmd, args = {}) => window.__TAURI__.core.invoke(cmd, args);

const VENDOR_LABELS = {
  other: 'other（通用 WebDAV）', nextcloud: 'nextcloud', owncloud: 'owncloud',
  sharepoint: 'sharepoint', rclone: 'rclone',
};
const VFS_LABELS = {
  off: 'off —— 不缓存（Office 可能报错）',
  minimal: 'minimal —— 只缓存读写中的文件',
  writes: 'writes —— 新增文件先落盘（推荐）',
  full: 'full —— 全量本地缓存',
};

let state = { profiles: [], mounts: [], status: null, autostart: null };

function escapeHtml(value) {
  return String(value == null ? '' : value)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

let toastTimer = null;
function toast(message, isError = false) {
  const node = $('toast');
  node.textContent = message;
  node.className = 'toast show' + (isError ? ' err' : '');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { node.className = 'toast'; }, isError ? 6000 : 2600);
}

async function run(label, fn) {
  try {
    const result = await fn();
    if (label) toast(label);
    return result;
  } catch (err) {
    toast(`${label ? label + '失败：' : ''}${err}`, true);
    return null;
  }
}

function badge(text, kind) {
  return `<span class="badge ${kind}">${escapeHtml(text)}</span>`;
}

function fillSelect(node, values, selected, labeler) {
  node.innerHTML = values.map((value) =>
    `<option value="${escapeHtml(value)}"${value === selected ? ' selected' : ''}>${
      escapeHtml(labeler ? labeler(value) : value)}</option>`).join('');
}

function renderEnv(status) {
  const engine = status.engine || {};
  const rows = [];
  rows.push(engine.installed ? badge('rclone 已找到', 'ok') : badge('未找到 rclone', 'err'));
  rows.push(engine.running
    ? badge(engine.version ? `引擎运行中 ${engine.version}` : '引擎运行中', 'ok')
    : badge('引擎未启动', 'mute'));
  rows.push(badge(`密钥：${status.secrets || '-'}`, 'mute'));
  $('env').innerHTML = rows.join('');
}

function renderAutostart(info) {
  const box = $('autostart-box');
  const rows = [['状态', info && info.installed ? badge('已注册', 'ok') : badge('未注册', 'mute')]];
  if (info && info.installed) {
    rows.push(['模式', escapeHtml(info.mode === 'boot' ? '开机（SYSTEM）' : '登录（当前用户）')]);
    rows.push(['任务名', `<span class="muted">${escapeHtml(info.task_name || '')}</span>`]);
  }
  box.innerHTML = rows.map(([k, v]) => `<div><span class="muted">${k}</span><span>${v}</span></div>`).join('');
}

function renderProfiles() {
  const box = $('profiles');
  if (!state.profiles.length) {
    box.innerHTML = '<p class="muted">还没有连接。在左侧填好 WebDAV 地址和账号密码，保存即可。</p>';
    return;
  }
  const mounted = new Set((state.mounts || []).map((m) => (m.mount_point || '').toLowerCase()));
  box.innerHTML = state.profiles.map((profile) => {
    const isMounted = mounted.has((profile.drive || '').toLowerCase());
    return `
      <div class="profile" data-id="${escapeHtml(profile.id)}">
        <h3>${escapeHtml(profile.name)} ${isMounted ? badge('已挂载', 'ok') : badge('未挂载', 'mute')}
          ${profile.autostart ? badge('开机自启', 'ok') : ''}
          ${profile.read_only ? badge('只读', 'warn') : ''}
          ${profile.network_mode ? badge('网络驱动器', 'mute') : ''}
        </h3>
        <dl>
          <dt>地址</dt><dd>${escapeHtml(profile.url)}</dd>
          <dt>账号</dt><dd>${escapeHtml(profile.user || '(匿名)')} · ${profile.password_set ? '已保存密码' : '无密码'}</dd>
          <dt>挂载点</dt><dd>${escapeHtml(profile.drive)} · 卷标 ${escapeHtml(profile.volname || profile.name)}</dd>
          <dt>缓存</dt><dd>${escapeHtml(profile.vfs_cache_mode)} · dir-cache ${escapeHtml(profile.dir_cache_time)}</dd>
          ${profile.extra_opts ? `<dt>附加</dt><dd>${escapeHtml(profile.extra_opts)}</dd>` : ''}
        </dl>
        <div class="actions">
          ${isMounted
            ? '<button data-act="unmount">卸载</button>'
            : '<button class="primary" data-act="mount">挂载</button>'}
          <button data-act="test">测试连接</button>
          <button class="ghost" data-act="edit">编辑</button>
          <button class="ghost danger" data-act="delete">删除</button>
        </div>
      </div>`;
  }).join('');
}

function fillForm(profile) {
  const p = profile || {};
  $('f-id').value = p.id || '';
  $('f-name').value = p.name || '';
  $('f-url').value = p.url || '';
  $('f-user').value = p.user || '';
  $('f-password').value = '';
  $('f-password').placeholder = p.password_set ? '留空表示不修改' : '（可留空）';
  $('f-vendor').value = p.vendor || 'other';
  $('f-drive').value = p.drive || 'X:';
  $('f-volname').value = p.volname || '';
  $('f-vfs').value = p.vfs_cache_mode || 'writes';
  $('f-dircache').value = p.dir_cache_time || '5m';
  $('f-network').checked = !!p.network_mode;
  $('f-readonly').checked = !!p.read_only;
  $('f-autostart').checked = p.autostart !== undefined ? !!p.autostart : true;
  $('f-extra').value = p.extra_opts || '';
  $('form-title').textContent = p.id ? `编辑：${p.name}` : '新建连接';
}

function readForm() {
  return {
    id: $('f-id').value || null,
    name: $('f-name').value.trim(),
    url: $('f-url').value.trim(),
    user: $('f-user').value,
    password: $('f-password').value,
    vendor: $('f-vendor').value,
    drive: $('f-drive').value,
    volname: $('f-volname').value,
    vfs_cache_mode: $('f-vfs').value,
    dir_cache_time: $('f-dircache').value.trim() || '5m',
    network_mode: $('f-network').checked,
    read_only: $('f-readonly').checked,
    autostart: $('f-autostart').checked,
    extra_opts: $('f-extra').value.trim(),
  };
}

async function refresh() {
  const [status, profiles, autostart] = await Promise.all([
    invoke('app_status'),
    invoke('list_connections'),
    invoke('autostart_status').catch(() => null),
  ]);
  state.status = status;
  state.mounts = status.mounts || [];
  state.profiles = profiles || [];
  state.autostart = autostart;
  renderEnv(status);
  renderAutostart(autostart);
  renderProfiles();
}

async function boot() {
  fillSelect($('f-vendor'), ['other', 'nextcloud', 'owncloud', 'sharepoint', 'rclone'], 'other', (v) => VENDOR_LABELS[v] || v);
  fillSelect($('f-vfs'), ['off', 'minimal', 'writes', 'full'], 'writes', (v) => VFS_LABELS[v] || v);

  await run('', refresh);

  $('form').addEventListener('submit', async (event) => {
    event.preventDefault();
    const payload = readForm();
    if (!payload.name || !payload.url) return toast('名称和地址都要填', true);
    const saved = await run('保存', () => invoke('save_connection', { input: payload }));
    if (saved) { fillForm(saved); await refresh(); }
  });
  $('btn-reset').addEventListener('click', () => fillForm(null));
  $('btn-refresh').addEventListener('click', () => run('已刷新', refresh));
  $('btn-logs').addEventListener('click', async () => {
    const panel = $('log-panel');
    panel.hidden = !panel.hidden;
    if (!panel.hidden) {
      const lines = await run('', () => invoke('logs', { limit: 300 }));
      if (lines) $('log').textContent = lines.join('\n');
    }
  });

  $('btn-autostart-on').addEventListener('click', async () => {
    const mode = $('autostart-mode').value;
    const result = await run('已注册开机自启', () => invoke('install_autostart', { mode }));
    if (result) await refresh();
  });
  $('btn-autostart-off').addEventListener('click', async () => {
    const result = await run('已移除开机自启', () => invoke('uninstall_autostart'));
    if (result) await refresh();
  });

  $('profiles').addEventListener('click', async (event) => {
    const button = event.target.closest('button[data-act]');
    if (!button) return;
    const id = button.closest('.profile').dataset.id;
    const profile = state.profiles.find((p) => p.id === id);
    const act = button.dataset.act;
    if (act === 'edit') return fillForm(profile);
    if (act === 'delete') {
      if (!confirm(`删除「${profile.name}」？会一并卸载并移除引擎侧配置。`)) return;
      const done = await run('已删除', () => invoke('delete_connection', { id }));
      if (done !== null) { fillForm(null); await refresh(); }
      return;
    }
    button.disabled = true;
    try {
      if (act === 'mount') await run('已挂载', () => invoke('mount_connection', { id }));
      else if (act === 'unmount') await run('已卸载', () => invoke('unmount_connection', { id }));
      else if (act === 'test') {
        const report = await run('', () => invoke('probe_connection', { id }));
        if (report) {
          const sample = (report.sample || []).join('、');
          toast(`连接成功，根目录 ${report.entries} 项${sample ? '：' + sample : ''}`);
        }
      }
    } finally {
      button.disabled = false;
      await refresh();
    }
  });

  setInterval(() => { if (!document.hidden) refresh().catch(() => {}); }, 5000);
}

boot().catch((err) => toast(`初始化失败：${err}`, true));
