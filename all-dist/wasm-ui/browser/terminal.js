(() => {
  let terminal;
  let fit;
  let frame;
  window.DevtoolsTerminal = {
    create(onData, onResize) {
      if (!window.Terminal || !window.FitAddon) throw new Error('Lokale terminalbibliotheek ontbreekt');
      terminal = new window.Terminal({
        cursorBlink: !matchMedia('(prefers-reduced-motion: reduce)').matches,
        cursorStyle: 'bar',
        fontSize: matchMedia('(max-width: 650px)').matches ? 12 : 13,
        fontFamily: "ui-monospace, 'Cascadia Code', 'Liberation Mono', Menlo, Consolas, monospace",
        lineHeight: 1.35,
        scrollback: 5000,
        screenReaderMode: true,
        theme: { background: '#0d120f', foreground: '#dce6d8', cursor: '#b7d993', selectionBackground: '#45603788', black: '#0d120f', red: '#eea28f', green: '#b7d993', yellow: '#e4ca8b', blue: '#9abec2', magenta: '#c9afd0', cyan: '#a4cfbd', white: '#e1e8df', brightBlack: '#8b9e87', brightRed: '#ffc0ad', brightGreen: '#ccebaa', brightYellow: '#f1dca7', brightBlue: '#b8d3da', brightMagenta: '#ddc5e6', brightCyan: '#c4e5d6', brightWhite: '#f2f7ef' }
      });
      fit = new window.FitAddon.FitAddon();
      terminal.loadAddon(fit);
      terminal.open(document.getElementById('terminal'));
      terminal.onData(onData);
      terminal.onTitleChange(title => { document.getElementById('terminal').title = title; });
      const resize = () => {
        cancelAnimationFrame(frame);
        frame = requestAnimationFrame(() => {
          document.documentElement.style.setProperty('--app-height', `${window.visualViewport?.height || window.innerHeight}px`);
          if (!document.getElementById('terminal').clientWidth) return;
          fit.fit();
          onResize(terminal.cols, terminal.rows);
        });
      };
      new ResizeObserver(resize).observe(document.getElementById('terminal'));
      window.addEventListener('resize', resize);
      window.visualViewport?.addEventListener('resize', resize);
      resize();
    },
    write(bytes) { terminal.write(bytes); },
    reset() { terminal.reset(); },
    focus() { terminal.focus(); },
    fit() {
      fit.fit();
      return [terminal.cols, terminal.rows];
    }
  };
})();
