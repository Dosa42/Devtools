import init from './wasm/devtools_ui.js';

try {
  await init();
} catch (error) {
  const target = document.getElementById('terminal-error') || document.getElementById('wasm-root');
  target.hidden = false;
  target.textContent = `WASM-interface kon niet starten: ${error.message}. Bouw de UI met python devtool/ui/build.py.`;
  target.setAttribute('role', 'alert');
}
