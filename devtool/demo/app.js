(() => {
  'use strict';
  const element = id => document.getElementById(id);
  const storageKey = 'devtool-terminal-session';
  const state = { session: null, cursor: 0, settingsSignature: '', filesSignature: '', selected: null, busy: false, queue: Promise.resolve(), ready: false, latestRoot: null };
  let terminal;
  let fit;
  let refreshRunning = false;
  let fitFrame = 0;
  let telemetrySignature = '';
  let telemetryRunning = false;

  async function api(path, body) {
    const response = await fetch(path, body === undefined ? {} : { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
    const result = await response.json();
    if (!response.ok) throw new Error(result.error || `HTTP ${response.status}`);
    return result;
  }

  function connection(text, kind) {
    if (element('connection').textContent !== text) element('connection').textContent = text;
    element('connection').dataset.state = kind;
  }

  function bytesToBase64(bytes) {
    let text = '';
    for (let offset = 0; offset < bytes.length; offset += 8192) text += String.fromCharCode(...bytes.subarray(offset, offset + 8192));
    return btoa(text);
  }

  function send(text) {
    const id = state.session;
    state.queue = state.queue.catch(() => {}).then(() => api('/api/input', { session: id, data: bytesToBase64(new TextEncoder().encode(text)) })).catch(error => {
      element('activity').textContent = error.message;
      connection('Verbinding verbroken', 'error');
    });
    return state.queue;
  }

  function prepare(text) {
    if (!state.ready) return;
    send(text);
    terminal.focus();
    element('activity').textContent = 'Opdracht ingevoegd · Enter voert uit';
  }

  function quote(value) {
    return `'${String(value).replaceAll("'", "'\\''")}'`;
  }

  function commandText() {
    const parts = ['devtool compress'];
    const source = element('compression-source').value;
    const format = element('compression-format').value;
    const output = element('compression-output').value;
    if (source) parts.push(quote(source));
    if (format) parts.push('--format', format);
    if (output) parts.push('-o', quote(output));
    return parts.join(' ');
  }

  function displayPath(path) {
    if (!path || !state.workspace) return path || '~/workspace';
    if (path === state.workspace) return '~/workspace';
    if (path.startsWith(state.workspace + '/')) return '~/workspace/' + path.slice(state.workspace.length + 1);
    return path;
  }

  function setReady(ready, alive) {
    state.ready = ready && alive;
    for (const button of document.querySelectorAll('[data-command], .catalog button')) button.disabled = !state.ready;
    element('compression-form').querySelector('button').disabled = !state.ready;
    if (!alive) {
      connection('Shell beëindigd', 'error');
      element('activity').textContent = 'De shell is afgesloten. Nieuwe sessie opent een nieuwe terminal.';
    }
  }

  function renderSettings(snapshot) {
    const signature = JSON.stringify([snapshot.settings, snapshot.settings_error]);
    if (signature === state.settingsSignature) return;
    state.settingsSignature = signature;
    const target = element('settings-values');
    target.replaceChildren();
    if (!snapshot.settings) {
      const error = document.createElement('span');
      error.textContent = snapshot.settings_error || 'Instellingen niet beschikbaar';
      error.className = 'setting';
      target.append(error);
      return;
    }
    const settings = snapshot.settings;
    const values = [
      ['parents', settings.filesystem.create_parents],
      ['file', settings.filesystem.existing_file],
      ['archive', settings.compression.format],
      ['level', settings.compression.level],
      ['output', settings.output.format],
      ['color', settings.output.color],
      ['schema', settings.schema_version]
    ];
    for (const [name, value] of values) {
      const entry = document.createElement('span');
      entry.className = 'setting';
      const label = document.createElement('span');
      label.textContent = name;
      const content = document.createElement('b');
      content.textContent = String(value);
      entry.append(label, content);
      target.append(entry);
    }
    target.title = snapshot.config_path;
  }

  function bytesLabel(value) {
    if (value < 1024) return `${value} B`;
    if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} K`;
    return `${(value / 1024 / 1024).toFixed(1)} M`;
  }

  function node(tag, className, text) {
    const target = document.createElement(tag);
    if (className) target.className = className;
    if (text !== undefined) target.textContent = text;
    return target;
  }

  function duration(value) {
    if (value === null) return 'duur onbekend';
    return value < 1000 ? `${value} ms` : `${(value / 1000).toFixed(1)} s`;
  }

  function clock(value) {
    return new Date(value * 1000).toLocaleTimeString('nl-BE', { hour12: false });
  }

  function renderTelemetry(data) {
    const status = element('telemetry-status');
    const stale = data.shell_alive && (!data.sampled_at || Date.now() / 1000 - data.sampled_at > 3);
    status.dataset.state = stale || data.coverage !== 'complete' ? 'error' : data.shell_alive ? 'live' : 'closed';
    const label = stale ? 'Verouderd' : data.coverage !== 'complete' ? 'Beperkte meting' : data.shell_alive ? 'Live' : 'Afgesloten';
    if (status.textContent !== label) status.textContent = label;
    element('telemetry-updated').textContent = data.sampled_at ? clock(data.sampled_at) : 'Nog geen meting';
    element('telemetry-source-label').textContent = data.sources.includes('ps') ? 'Bash-events + macOS ps' : 'Bash-events + /proc';
    const signature = JSON.stringify([data.processes, data.commands, data.executables, data.shell_ready, data.shell_alive, data.watch_running]);
    if (signature === telemetrySignature) return;
    telemetrySignature = signature;
    const current = data.commands.find(command => command.status === 'running');
    element('telemetry-mode').textContent = !data.shell_alive ? 'Afgesloten' : current ? 'Command draait' : data.shell_ready ? 'Prompt gereed' : 'Bash actief';
    element('telemetry-current-command').textContent = current ? current.command : !data.shell_alive ? 'Bash is afgesloten' : 'Bash wacht op een command';
    const meta = element('telemetry-current-meta');
    meta.replaceChildren(node('span', '', `Bash PID ${data.shell_pid}`), node('span', '', data.watch_running ? 'Watch actief' : 'Watch uit'));
    if (current) meta.append(node('span', '', duration(current.duration_ms)));
    const engines = data.executables.map((engine, index) => {
      const row = node('div', 'engine-row');
      row.dataset.active = String(engine.active);
      const content = node('div');
      const name = node('strong', 'engine-name', engine.name);
      name.title = engine.path;
      const note = engine.active ? `PID ${engine.pids.join(', ')} · ${engine.name === 'devtools-watch' ? 'achtergrond' : 'proces actief'}` : engine.last_used ? `Laatst gebruikt ${clock(engine.last_used)}` : engine.name === 'devtools-watch' ? 'Monitor gestopt' : 'Beschikbaar in Bash';
      content.append(name, node('span', 'engine-note', note));
      row.append(node('span', 'engine-index', String(index + 1).padStart(2, '0')), content, node('span', 'engine-state', engine.active ? 'actief' : engine.name === 'devtools-watch' ? 'uit' : 'gereed'));
      return row;
    });
    element('telemetry-engines').replaceChildren(...engines);
    element('telemetry-process-count').textContent = `${data.processes.length} actief`;
    const processes = data.processes.map(process => {
      const row = node('div', 'process-row');
      const top = node('div', 'process-top');
      top.append(node('strong', '', process.name), node('span', '', `PID ${process.pid}`));
      const layer = process.layer === 'watch' ? 'Watch-laag' : process.pid === data.shell_pid ? 'Bash / PTY' : 'Bash-child';
      const foreground = process.tty && process.group === process.foreground_group ? 'voorgrond' : 'achtergrond';
      const states = { R: 'draait', S: 'wacht', D: 'I/O', T: 'gestopt', I: 'idle' };
      row.append(top, node('div', 'process-meta', `${layer} · ${foreground} · ${states[process.state] || process.state} · ${bytesLabel(process.rss_bytes)}`));
      if (process.pid !== data.shell_pid) {
        const args = node('code', 'process-args', process.arguments.join(' '));
        args.title = process.executable || '';
        row.append(args);
      }
      return row;
    });
    element('telemetry-processes').replaceChildren(...(processes.length ? processes : [node('p', 'telemetry-empty', 'Geen actieve sessieprocessen gemeten.')]));
    element('telemetry-command-count').textContent = `${data.commands.length} / ${data.history_limit}`;
    const history = data.commands.slice(0, 30).map(command => {
      const entry = node('li', 'history-entry');
      entry.dataset.status = command.status;
      entry.dataset.failed = String(command.exit_code !== null && command.exit_code !== 0);
      const result = command.status === 'running' ? 'draait' : command.status === 'interrupted' ? 'geen eindstatus' : `exit ${command.exit_code}`;
      const top = node('div', 'history-top');
      top.append(node('time', '', clock(command.started)), node('span', 'history-result', `${result} · ${duration(command.duration_ms)}`));
      const text = node('code', '', command.command);
      entry.append(top, text);
      const executables = command.executables.length ? command.executables.join(' → ') : 'Bash / terminalcommand';
      entry.append(node('div', 'history-executables', executables));
      return entry;
    });
    element('telemetry-history').replaceChildren(...(history.length ? history : [node('li', 'telemetry-empty', 'Nog geen commands uitgevoerd in deze sessie.')]));
  }

  async function refreshTelemetry() {
    if (!state.session || telemetryRunning) return;
    telemetryRunning = true;
    const session = state.session;
    try {
      const data = await api(`/api/telemetry?session=${session}`);
      if (session === state.session) renderTelemetry(data);
    } catch (error) {
      if (session === state.session) {
        if (element('telemetry-status').textContent !== 'Geen verbinding') element('telemetry-status').textContent = 'Geen verbinding';
        element('telemetry-status').dataset.state = 'error';
        element('telemetry-status').title = error.message;
      }
    } finally {
      telemetryRunning = false;
    }
  }

  function showInspector(view) {
    const telemetry = view === 'telemetry';
    element('telemetry-panel').hidden = !telemetry;
    element('file-panel').hidden = telemetry;
    element('telemetry-tab').setAttribute('aria-pressed', String(telemetry));
    element('files-tab').setAttribute('aria-pressed', String(!telemetry));
    fitTerminal();
  }

  function renderFiles(entries) {
    const signature = JSON.stringify(entries);
    if (signature === state.filesSignature) return;
    state.filesSignature = signature;
    const tree = element('file-tree');
    const files = entries.filter(entry => !entry.directory);
    element('file-count').textContent = `${files.length} bestanden`;
    if (entries.length === 0) {
      tree.innerHTML = '<div class="files-empty"><span aria-hidden="true">└─</span><p>De werkmap is nog leeg.</p><p class="muted">Begin met <code>devtool -D</code>.</p></div>';
      return;
    }
    const fragment = document.createDocumentFragment();
    function level(parent, depth) {
      const children = entries.filter(entry => {
        const offset = entry.path.lastIndexOf('/');
        return (offset === -1 ? '' : entry.path.slice(0, offset)) === parent;
      }).sort((left, right) => Number(right.directory) - Number(left.directory) || left.name.localeCompare(right.name));
      for (const entry of children) {
        const row = document.createElement('button');
        row.type = 'button';
        row.className = `file-row${entry.directory ? ' directory' : ''}`;
        row.style.paddingInlineStart = `${6 + depth * 13}px`;
        row.dataset.selected = String(state.selected === entry.path);
        row.title = entry.path;
        const icon = document.createElement('span');
        icon.className = 'file-symbol';
        icon.textContent = entry.directory ? '▾' : entry.symlink ? '↗' : '·';
        icon.setAttribute('aria-hidden', 'true');
        const name = document.createElement('span');
        name.className = 'file-name';
        name.textContent = entry.name;
        row.append(icon, name);
        if (!entry.directory) {
          const size = document.createElement('span');
          size.className = 'file-size';
          size.textContent = bytesLabel(entry.bytes);
          row.append(size);
          row.addEventListener('click', () => previewFile(entry));
        } else {
          row.addEventListener('click', () => {
            element('compression-source').value = state.workspace + '/' + entry.path;
            updateCompression();
            togglePanel('compression-panel', 'compression-toggle', true);
          });
        }
        fragment.append(row);
        if (entry.directory && !entry.symlink) level(entry.path, depth + 1);
      }
    }
    level('', 0);
    tree.replaceChildren(fragment);
  }

  async function previewFile(entry) {
    state.selected = entry.path;
    element('preview-name').textContent = entry.name;
    const url = `/api/file?session=${state.session}&path=${encodeURIComponent(entry.path)}`;
    element('download-file').href = url;
    element('file-preview').hidden = false;
    const target = element('preview-content');
    target.replaceChildren();
    try {
      if (entry.name.toLowerCase().endsWith('.pdf')) {
        const frame = document.createElement('iframe');
        frame.src = url;
        frame.title = entry.name;
        target.append(frame);
      } else {
        const response = await fetch(url);
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const bytes = new Uint8Array(await response.arrayBuffer());
        const pre = document.createElement('pre');
        const binary = entry.name.endsWith('.7z') || entry.name.endsWith('.gz') || bytes.slice(0, 2048).includes(0);
        if (binary) {
          pre.textContent = `${bytes.length} bytes\n\n` + Array.from(bytes.slice(0, 256), value => value.toString(16).padStart(2, '0')).join(' ');
        } else {
          pre.textContent = bytes.length ? new TextDecoder().decode(bytes.slice(0, 16384)) : '(leeg bestand)';
          if (bytes.length > 16384) pre.textContent += `\n\n… voorbeeld van 16384 / ${bytes.length} bytes`;
        }
        target.append(pre);
      }
    } catch (error) {
      target.textContent = error.message;
    }
  }

  function catalog(schema) {
    const target = element('catalog-panel');
    const grouped = new Map();
    function walk(command, prefix) {
      const path = prefix ? `${prefix} ${command.name}` : command.name;
      const nested = command.commands || [];
      if (nested.length) {
        for (const child of nested) walk(child, path);
      } else {
        const group = path.split(' ')[1] || 'devtool';
        if (!grouped.has(group)) grouped.set(group, []);
        grouped.get(group).push({ path, about: command.about });
      }
    }
    walk(schema.cli, '');
    for (const [name, commands] of grouped) {
      const group = document.createElement('section');
      const heading = document.createElement('h2');
      heading.textContent = name;
      group.append(heading);
      for (const command of commands) {
        const button = document.createElement('button');
        button.type = 'button';
        button.textContent = command.path;
        button.title = command.about || command.path;
        button.addEventListener('click', () => prepare(command.path + ' '));
        group.append(button);
      }
      target.append(group);
    }
  }

  async function refresh() {
    if (!state.session || refreshRunning) return;
    refreshRunning = true;
    const session = state.session;
    try {
      const [snapshot, files] = await Promise.all([api(`/api/state?session=${session}`), api(`/api/files?session=${session}`)]);
      if (session !== state.session) return;
      state.workspace = snapshot.workspace;
      state.latestRoot = snapshot.last_root;
      element('cwd').textContent = displayPath(snapshot.cwd);
      element('cwd').title = snapshot.cwd;
      element('run-status').textContent = snapshot.last_run ? `run-nr.${String(snapshot.last_run).padStart(3, '0')}` : 'Nog geen hoofdmap';
      renderSettings(snapshot);
      renderFiles(files.entries);
      setReady(snapshot.ready, snapshot.alive);
    } catch (error) {
      element('activity').textContent = error.message;
    } finally {
      refreshRunning = false;
    }
  }

  async function outputLoop() {
    while (true) {
      const session = state.session;
      if (session) {
        try {
          const output = await api(`/api/output?session=${session}&cursor=${state.cursor}`);
          if (session === state.session) {
            if (output.reset) terminal.reset();
            if (output.data) terminal.write(Uint8Array.from(atob(output.data), character => character.charCodeAt(0)));
            state.cursor = output.cursor;
            setReady(output.ready, output.alive);
            if (output.alive) connection('Verbonden', 'connected');
          }
        } catch (error) {
          connection('Opnieuw verbinden…', 'error');
          element('activity').textContent = error.message;
        }
      }
      await new Promise(resolve => setTimeout(resolve, 140));
    }
  }

  function fitTerminal() {
    cancelAnimationFrame(fitFrame);
    fitFrame = requestAnimationFrame(() => {
      document.documentElement.style.setProperty('--app-height', `${window.visualViewport?.height || window.innerHeight}px`);
      if (!terminal || !state.session) return;
      fit.fit();
      element('terminal-size').textContent = `${terminal.cols} × ${terminal.rows} · BASH PTY`;
      api('/api/resize', { session: state.session, columns: terminal.cols, rows: terminal.rows }).catch(error => { element('activity').textContent = error.message; });
    });
  }

  async function startSession(existing) {
    setReady(false, true);
    const session = await api('/api/session', existing ? { session: existing } : {});
    state.session = session.id;
    state.cursor = 0;
    state.workspace = session.workspace;
    state.settingsSignature = '';
    state.filesSignature = '';
    telemetrySignature = '';
    state.selected = null;
    localStorage.setItem(storageKey, session.id);
    element('session-label').textContent = session.id.slice(0, 6);
    element('session-label').title = session.workspace;
    terminal.reset();
    element('file-preview').hidden = true;
    connection('Verbonden', 'connected');
    element('activity').textContent = 'Terminal gereed';
    await refresh();
    await refreshTelemetry();
    fitTerminal();
  }

  function togglePanel(panel, button, force) {
    const open = force === undefined ? element(panel).hidden : force;
    element(panel).hidden = !open;
    element(button).setAttribute('aria-expanded', String(open));
    fitTerminal();
  }

  function updateCompression() {
    element('compression-preview').textContent = commandText();
  }

  async function initialize() {
    if (!window.Terminal || !window.FitAddon) throw new Error('De lokale terminalbibliotheek kon niet worden geladen.');
    terminal = new window.Terminal({
      cursorBlink: true,
      cursorStyle: 'bar',
      fontSize: matchMedia('(max-width: 650px)').matches ? 12 : 13,
      fontFamily: "ui-monospace, 'Cascadia Code', 'Liberation Mono', Menlo, Consolas, monospace",
      lineHeight: 1.35,
      scrollback: 5000,
      allowProposedApi: false,
      screenReaderMode: true,
      theme: { background: '#0d120f', foreground: '#dce6d8', cursor: '#b7d993', selectionBackground: '#45603788', black: '#0d120f', red: '#eea28f', green: '#b7d993', yellow: '#e4ca8b', blue: '#9abec2', magenta: '#c9afd0', cyan: '#a4cfbd', white: '#e1e8df', brightBlack: '#8b9e87', brightRed: '#ffc0ad', brightGreen: '#ccebaa', brightYellow: '#f1dca7', brightBlue: '#b8d3da', brightMagenta: '#ddc5e6', brightCyan: '#c4e5d6', brightWhite: '#f2f7ef' }
    });
    fit = new window.FitAddon.FitAddon();
    terminal.loadAddon(fit);
    terminal.open(element('terminal'));
    terminal.onData(send);
    terminal.onTitleChange(title => { element('terminal').title = title; });
    document.querySelectorAll('[data-command]').forEach(button => button.addEventListener('click', () => prepare(button.dataset.command)));
    element('compression-toggle').addEventListener('click', () => togglePanel('compression-panel', 'compression-toggle'));
    element('catalog-toggle').addEventListener('click', () => togglePanel('catalog-panel', 'catalog-toggle'));
    for (const name of ['compression-source', 'compression-format', 'compression-output']) element(name).addEventListener('input', updateCompression);
    element('compression-form').addEventListener('submit', event => { event.preventDefault(); prepare(commandText()); });
    element('line-form').addEventListener('submit', event => {
      event.preventDefault();
      send(element('line-input').value + '\r');
      element('line-input').value = '';
      terminal.focus();
    });
    const keys = { tab: '\t', up: '\x1b[A', down: '\x1b[B', interrupt: '\x03', eof: '\x04' };
    document.querySelectorAll('[data-key]').forEach(button => button.addEventListener('click', () => { send(keys[button.dataset.key]); terminal.focus(); }));
    element('new-session').addEventListener('click', async () => {
      try { await startSession(); } catch (error) { element('activity').textContent = error.message; }
    });
    element('refresh-files').addEventListener('click', () => { state.filesSignature = ''; refresh(); });
    element('close-preview').addEventListener('click', () => { element('file-preview').hidden = true; state.selected = null; });
    element('files-toggle').addEventListener('click', () => {
      const window = document.querySelector('.terminal-window');
      const open = !window.classList.toggle('inspector-closed');
      element('files-toggle').setAttribute('aria-expanded', String(open));
      fitTerminal();
    });
    element('telemetry-tab').addEventListener('click', () => showInspector('telemetry'));
    element('files-tab').addEventListener('click', () => showInspector('files'));
    const meta = await api('/api/meta');
    element('version').textContent = meta.version.replace('devtool ', '');
    document.querySelector('.page-footer > span').firstChild.textContent = meta.version + ' ';
    catalog(meta.schema);
    const existing = localStorage.getItem(storageKey);
    await startSession(existing);
    new ResizeObserver(fitTerminal).observe(element('terminal'));
    const viewport = () => { document.documentElement.style.setProperty('--app-height', `${window.visualViewport?.height || window.innerHeight}px`); fitTerminal(); };
    window.visualViewport?.addEventListener('resize', viewport);
    window.addEventListener('resize', viewport);
    viewport();
    outputLoop();
    setInterval(refresh, 900);
    setInterval(refreshTelemetry, 350);
  }

  initialize().catch(error => {
    connection('Fout bij opstarten', 'error');
    element('terminal-error').hidden = false;
    element('terminal-error').textContent = error.message;
    element('activity').textContent = error.message;
  });
})();
