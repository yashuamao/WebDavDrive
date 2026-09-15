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
const VFS_SHORT_LABELS = {
  off: '关闭', minimal: '最小', writes: '写入', full: '完整',
};

let state = {
  profiles: [], mounts: [], status: null, autostart: null,
  selectedId: null, activeView: 'drives',
};

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

function mountFor(profile) {
  const configuredPoint = (profile.drive || '').toLowerCase();
  const configuredRemote = `${profile.remote || ''}:`.toLowerCase();
  return (state.mounts || []).find((mount) =>
    (mount.mount_point || '').toLowerCase() === configuredPoint
      || (mount.fs || '').toLowerCase() === configuredRemote) || null;
}

function displayDrive(profile, mount) {
  const value = (mount && mount.mount_point) || profile.drive || '*';
  const match = /^([a-z]):/i.exec(value);
  return match ? `${match[1].toUpperCase()}:` : 'DAV';
}

function profileState(profile, mount) {
  if (mount) return { text: '已连接', kind: 'ok' };
  if (profile.read_only) return { text: '只读', kind: 'warn' };
  return { text: '未连接', kind: 'mute' };
}

function renderEnv(status) {
  const engine = status.engine || {};
  const running = !!engine.running;
  const installed = !!engine.installed;
  const dotKind = running ? 'ok' : (installed ? 'mute' : 'err');
  const engineText = running
    ? `rclone 引擎运行中${engine.version ? ` · ${engine.version}` : ''}`
    : (installed ? 'rclone 引擎未启动' : '未找到 rclone');

  $('engine-dot').className = `status-dot ${dotKind}`;
  $('sidebar-engine-dot').className = `status-dot ${dotKind}`;
  $('engine-text').textContent = engineText;
  $('sidebar-engine-text').textContent = running ? '引擎运行中' : (installed ? '引擎未启动' : '缺少 rclone');

  $('env').innerHTML = [
    installed ? badge('rclone 已找到', 'ok') : badge('未找到 rclone', 'err'),
    running ? badge('引擎运行中', 'ok') : badge('引擎未启动', 'mute'),
    badge(`凭据保护：${status.secrets || '-'}`, 'mute'),
  ].join('');

  const hint = $('engine-hint');
  hint.hidden = installed;
  hint.textContent = '未找到 rclone.exe：把 rclone.exe 放到程序同目录（或 bin\\ 子目录），'
    + '或设置环境变量 RCLONE_EXE。下载：https://rclone.org/downloads/';
}

function renderAutostart(info) {
  const box = $('autostart-box');
  const rows = [['状态', info && info.installed ? badge('已注册', 'ok') : badge('未注册', 'mute')]];
  if (info && info.installed) {
    rows.push(['模式', escapeHtml(info.mode === 'boot' ? '开机（SYSTEM）' : '登录（当前用户）')]);
    rows.push(['任务名', `<span class="muted">${escapeHtml(info.task_name || '')}</span>`]);
  }
  box.innerHTML = rows.map(([key, value]) =>
    `<div><span class="muted">${key}</span><span>${value}</span></div>`).join('');
}

function renderProfiles() {
  const box = $('profiles');
  $('profile-count').textContent = `${state.profiles.length} 个连接`;

  if (!state.profiles.length) {
    state.selectedId = null;
    box.innerHTML = `
      <div class="empty-state">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16l2 10H2L4 7Z"/><path d="M7 13h10"/></svg>
        <strong>还没有连接</strong>
        <span>新建一个 WebDAV 连接后即可挂载到资源管理器。</span>
        <button class="button primary" type="button" data-act="new">新建连接</button>
      </div>`;
    renderProfileDetail();
    return;
  }

  if (!state.profiles.some((profile) => profile.id === state.selectedId)) {
    state.selectedId = state.profiles[0].id;
  }

  box.innerHTML = state.profiles.map((profile) => {
    const mount = mountFor(profile);
    const currentState = profileState(profile, mount);
    const selected = profile.id === state.selectedId;
    return `
      <article class="profile${selected ? ' selected' : ''}" data-id="${escapeHtml(profile.id)}"
        tabindex="0" role="button" aria-pressed="${selected}" title="${escapeHtml(profile.url)}">
        <div class="drive-tile" aria-hidden="true">${escapeHtml(displayDrive(profile, mount))}</div>
        <div class="profile-copy">
          <h2>${escapeHtml(profile.name)}</h2>
          <span class="profile-url">${escapeHtml(profile.url)}</span>
        </div>
        <div class="profile-meta">
          <span class="protocol">WebDAV</span>
          <span class="state-label ${currentState.kind}">
            <span class="status-dot ${currentState.kind}" aria-hidden="true"></span>
            ${currentState.text}
          </span>
        </div>
      </article>`;
  }).join('');

  renderProfileDetail();
}

function renderProfileDetail() {
  const box = $('profile-detail');
  const profile = state.profiles.find((item) => item.id === state.selectedId);
  if (!profile) {
    box.innerHTML = `
      <div class="detail-empty">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16l2 10H2L4 7Z"/><path d="M7 13h10"/></svg>
        <span>选择连接后查看详情</span>
      </div>`;
    return;
  }

  const mount = mountFor(profile);
  const currentState = profileState(profile, mount);
  const point = (mount && mount.mount_point) || profile.drive || '-';
  const user = profile.user || '匿名';
  const password = profile.password_set ? '已保存' : '未保存';

  box.innerHTML = `
    <div class="detail-identity">
      <div class="drive-tile" aria-hidden="true">${escapeHtml(displayDrive(profile, mount))}</div>
      <div>
        <h2>${escapeHtml(profile.name)}</h2>
        <span class="profile-url">${escapeHtml(profile.url)}</span>
        <span class="state-label ${currentState.kind}">
          <span class="status-dot ${currentState.kind}" aria-hidden="true"></span>
          ${currentState.text}
        </span>
      </div>
    </div>

    <section class="detail-section">
      <h3>连接信息</h3>
      <dl class="detail-list">
        <div><dt>协议</dt><dd>WebDAV</dd></div>
        <div><dt>账号</dt><dd>${escapeHtml(user)} · 密码${password}</dd></div>
        <div><dt>盘符</dt><dd>${escapeHtml(point)}</dd></div>
        <div><dt>网络驱动器</dt><dd>${profile.network_mode ? '是' : '否'}</dd></div>
      </dl>
    </section>

    <section class="detail-section">
      <h3>挂载选项</h3>
      <dl class="detail-list">
        <div><dt>VFS 缓存</dt><dd>${escapeHtml(VFS_SHORT_LABELS[profile.vfs_cache_mode] || profile.vfs_cache_mode)}</dd></div>
        <div><dt>目录缓存</dt><dd>${escapeHtml(profile.dir_cache_time)}</dd></div>
        <div><dt>自动挂载</dt><dd>${profile.autostart ? '已启用' : '未启用'}</dd></div>
        <div><dt>访问模式</dt><dd>${profile.read_only ? '只读' : '读写'}</dd></div>
      </dl>
    </section>

    <div class="detail-actions">
      ${mount
        ? `<button class="button primary" type="button" data-act="open">
             <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6h7l2 2h9v10H3V6Z"/></svg>
             在资源管理器中打开
           </button>
           <button class="button" type="button" data-act="unmount">断开连接</button>`
        : `<button class="button primary" type="button" data-act="mount">
             <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16l2 10H2L4 7Z"/><path d="M7 13h10"/></svg>
             挂载驱动器
           </button>`}
      <button class="button" type="button" data-act="test">测试连接</button>
    </div>
    <div class="detail-links">
      <button class="link-button" type="button" data-act="edit">编辑配置</button>
      <button class="link-button danger" type="button" data-act="delete">删除连接</button>
    </div>`;
}

function fillForm(profile) {
  const current = profile || {};
  $('f-id').value = current.id || '';
  $('f-name').value = current.name || '';
  $('f-url').value = current.url || '';
  $('f-user').value = current.user || '';
  $('f-password').value = '';
  $('f-password').placeholder = current.password_set ? '留空表示不修改' : '（可留空）';
  $('f-clear-password').checked = false;
  $('f-clear-password').disabled = !current.password_set;
  $('f-vendor').value = current.vendor || 'other';
  $('f-drive').value = current.drive || 'X:';
  $('f-volname').value = current.volname || '';
  $('f-vfs').value = current.vfs_cache_mode || 'writes';
  $('f-dircache').value = current.dir_cache_time || '5m';
  $('f-network').checked = !!current.network_mode;
  $('f-readonly').checked = !!current.read_only;
  $('f-autostart').checked = current.autostart !== undefined ? !!current.autostart : true;
  $('f-extra').value = current.extra_opts || '';
  $('form-title').textContent = current.id ? `编辑：${current.name}` : '新建连接';
}

function readForm() {
  return {
    id: $('f-id').value || null,
    name: $('f-name').value.trim(),
    url: $('f-url').value.trim(),
    user: $('f-user').value,
    password: $('f-password').value,
    clear_password: $('f-clear-password').checked,
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

function openConnectionDialog(profile) {
  fillForm(profile || null);
  const dialog = $('connection-dialog');
  if (!dialog.open) dialog.showModal();
  requestAnimationFrame(() => $('f-name').focus());
}

function closeConnectionDialog() {
  const dialog = $('connection-dialog');
  if (dialog.open) dialog.close();
}

async function refresh() {
  // 注意：这里不能查 autostart 状态——每次查询会启动两个 schtasks 进程，
  // 5 秒轮询会让系统不断创建控制台进程。自启状态只在启动/操作/手动刷新时查。
  const [status, profiles] = await Promise.all([
    invoke('app_status'),
    invoke('list_connections'),
  ]);
  state.status = status;
  state.mounts = status.mounts || [];
  state.profiles = profiles || [];
  renderEnv(status);
  renderProfiles();
}

async function refreshAutostart() {
  const autostart = await invoke('autostart_status').catch(() => null);
  state.autostart = autostart;
  renderAutostart(autostart);
  return autostart;
}

async function loadLogs() {
  const lines = await run('', () => invoke('logs', { limit: 500 }));
  if (lines) $('log').textContent = lines.length ? lines.join('\n') : '暂无日志';
}

function setView(name) {
  state.activeView = name;
  document.querySelectorAll('.view').forEach((view) => {
    const active = view.id === `view-${name}`;
    view.hidden = !active;
    view.classList.toggle('active', active);
  });
  document.querySelectorAll('.nav-item').forEach((button) => {
    const active = button.dataset.view === name;
    button.classList.toggle('active', active);
    if (active) button.setAttribute('aria-current', 'page');
    else button.removeAttribute('aria-current');
  });
  if (name === 'logs') loadLogs();
  if (name === 'autostart') refreshAutostart();
}

function showExitError(message) {
  setView('drives');
  $('exit-error-text').textContent = message || '请关闭正在使用虚拟硬盘的程序后重试。';
  $('exit-alert').hidden = false;
  $('btn-retry-exit').disabled = false;
  toast('退出已中止：部分虚拟硬盘未能卸载', true);
  refresh().catch(() => {});
}

async function retryExit() {
  $('exit-alert').hidden = true;
  $('btn-retry-exit').disabled = true;
  const result = await run('', () => invoke('exit_application'));
  if (result === null) {
    $('exit-alert').hidden = false;
    $('btn-retry-exit').disabled = false;
  }
}

async function handleProfileAction(action, button) {
  const profile = state.profiles.find((item) => item.id === state.selectedId);
  if (action === 'new') return openConnectionDialog(null);
  if (!profile) return;
  if (action === 'edit') return openConnectionDialog(profile);

  if (action === 'delete') {
    if (!confirm(`删除「${profile.name}」？会一并卸载并移除引擎侧配置。`)) return;
    const done = await run('已删除', () => invoke('delete_connection', { id: profile.id }));
    if (done !== null) {
      state.selectedId = null;
      await refresh();
    }
    return;
  }

  if (button) button.disabled = true;
  try {
    if (action === 'mount') await run('已挂载', () => invoke('mount_connection', { id: profile.id }));
    else if (action === 'unmount') await run('已断开', () => invoke('unmount_connection', { id: profile.id }));
    else if (action === 'open') await run('已打开资源管理器', () => invoke('open_connection', { id: profile.id }));
    else if (action === 'test') {
      const report = await run('', () => invoke('probe_connection', { id: profile.id }));
      if (report) {
        const sample = (report.sample || []).join('、');
        toast(`连接成功，根目录 ${report.entries} 项${sample ? '：' + sample : ''}`);
      }
    }
  } finally {
    if (button) button.disabled = false;
    if (action !== 'open') await refresh();
  }
}

async function boot() {
  fillSelect($('f-vendor'), ['other', 'nextcloud', 'owncloud', 'sharepoint', 'rclone'], 'other',
    (value) => VENDOR_LABELS[value] || value);
  fillSelect($('f-vfs'), ['off', 'minimal', 'writes', 'full'], 'writes',
    (value) => VFS_LABELS[value] || value);

  await run('', async () => { await refresh(); await refreshAutostart(); });

  document.querySelectorAll('.nav-item').forEach((button) => {
    button.addEventListener('click', () => setView(button.dataset.view));
  });

  $('profiles').addEventListener('click', (event) => {
    const actionButton = event.target.closest('[data-act="new"]');
    if (actionButton) return handleProfileAction('new', actionButton);
    const profileNode = event.target.closest('.profile');
    if (!profileNode) return;
    state.selectedId = profileNode.dataset.id;
    renderProfiles();
  });
  $('profiles').addEventListener('keydown', (event) => {
    if (event.key !== 'Enter' && event.key !== ' ') return;
    const profileNode = event.target.closest('.profile');
    if (!profileNode) return;
    event.preventDefault();
    state.selectedId = profileNode.dataset.id;
    renderProfiles();
  });

  $('profile-detail').addEventListener('click', (event) => {
    const button = event.target.closest('button[data-act]');
    if (button) handleProfileAction(button.dataset.act, button);
  });

  $('btn-new').addEventListener('click', () => openConnectionDialog(null));
  $('btn-dialog-close').addEventListener('click', closeConnectionDialog);
  $('btn-reset').addEventListener('click', closeConnectionDialog);
  $('connection-dialog').addEventListener('cancel', closeConnectionDialog);
  $('connection-dialog').addEventListener('click', (event) => {
    if (event.target === $('connection-dialog')) closeConnectionDialog();
  });

  $('form').addEventListener('submit', async (event) => {
    event.preventDefault();
    const payload = readForm();
    if (!payload.name || !payload.url) return toast('名称和地址都要填', true);
    const saved = await run('已保存', () => invoke('save_connection', { input: payload }));
    if (saved) {
      state.selectedId = saved.id;
      closeConnectionDialog();
      await refresh();
    }
  });
  $('f-password').addEventListener('input', () => {
    if ($('f-password').value) $('f-clear-password').checked = false;
  });
  $('f-clear-password').addEventListener('change', () => {
    if ($('f-clear-password').checked) $('f-password').value = '';
  });

  $('btn-refresh').addEventListener('click', () => run('已刷新', async () => {
    await refresh();
    await refreshAutostart();
    if (state.activeView === 'logs') await loadLogs();
  }));
  $('btn-logs').addEventListener('click', () => setView('logs'));
  $('btn-reload-logs').addEventListener('click', () => run('日志已刷新', loadLogs));
  $('btn-retry-exit').addEventListener('click', retryExit);
  $('btn-force-exit').addEventListener('click', async () => {
    const confirmed = confirm('强制退出会跳过卸载确认，正在写入的文件可能受损。仍要退出吗？');
    if (!confirmed) return;
    await run('', () => invoke('force_exit'));
  });

  $('btn-autostart-on').addEventListener('click', async () => {
    const mode = $('autostart-mode').value;
    const result = await run('已注册自动挂载', () => invoke('install_autostart', { mode }));
    if (result) { await refresh(); await refreshAutostart(); }
  });
  $('btn-autostart-off').addEventListener('click', async () => {
    const result = await run('已移除自动挂载', () => invoke('uninstall_autostart'));
    if (result) { await refresh(); await refreshAutostart(); }
  });

  if (window.__TAURI__.event && typeof window.__TAURI__.event.listen === 'function') {
    try {
      await window.__TAURI__.event.listen('exit-cleanup-failed', (event) => {
        showExitError(String(event.payload || ''));
      });
    } catch (err) {
      // 事件通知是退出失败提示的增强路径，不能让 ACL/运行时异常拖垮整个界面。
      console.warn('无法订阅退出清理失败事件：', err);
    }
  }

  setInterval(() => { if (!document.hidden) refresh().catch(() => {}); }, 5000);
}

boot().catch((err) => toast(`初始化失败：${err}`, true));
