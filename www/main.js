import * as monaco from 'monaco-editor';
import { Terminal } from 'xterm';
import { FitAddon } from '@xterm/addon-fit';
import 'xterm/css/xterm.css';
import { Go2RustCompiler } from './go2rust-compiler.js';
import { RustRunner, State } from './rust-runner.js';

function stateLabel(state) {
  switch (state) {
    case State.INITIALIZING: return 'VM initializing...';
    case State.DOWNLOADING: return 'VM downloading...';
    case State.BOOTING: return 'VM booting...';
    case State.READY: return 'VM ready';
    case State.COMPILING: return 'Compiling...';
    case State.ERROR: return 'VM error';
    default: return state;
  }
}

class HighlightManager {
  constructor(goEditor, rustEditor) {
    this.goEditor = goEditor;
    this.rustEditor = rustEditor;
    this.goDecorations = [];
    this.rustDecorations = [];
    this.result = null;
  }
  setResult(result) { this.result = result; }
  highlightFromGo(line, column) {
    if (!this.result || !this.result.success) { this.clearRustHighlight(); return; }
    const span = this.result.go_to_output(line, column);
    if (span.length === 4) {
      this.rustDecorations = this.rustEditor.deltaDecorations(this.rustDecorations, [{
        range: new monaco.Range(span[0], span[1], span[2], span[3]),
        options: { className: 'source-map-highlight', isWholeLine: false },
      }]);
    } else { this.clearRustHighlight(); }
  }
  highlightFromRust(line, column) {
    if (!this.result || !this.result.success) { this.clearGoHighlight(); return; }
    const span = this.result.output_to_go(line, column);
    if (span.length === 4) {
      this.goDecorations = this.goEditor.deltaDecorations(this.goDecorations, [{
        range: new monaco.Range(span[0], span[1], span[2], span[3]),
        options: { className: 'source-map-highlight', isWholeLine: false },
      }]);
    } else { this.clearGoHighlight(); }
  }
  clearGoHighlight() { this.goDecorations = this.goEditor.deltaDecorations(this.goDecorations, []); }
  clearRustHighlight() { this.rustDecorations = this.rustEditor.deltaDecorations(this.rustDecorations, []); }
}

function setupCopyButton(buttonId, getContent) {
  const button = document.getElementById(buttonId);
  if (!button) return;
  button.addEventListener('click', async () => {
    const content = getContent();
    if (!content) return;
    try {
      await navigator.clipboard.writeText(content);
      button.classList.add('copied');
      button.querySelector('span').textContent = 'Copied!';
      setTimeout(() => {
        button.classList.remove('copied');
        button.querySelector('span').textContent = 'Copy';
      }, 2000);
    } catch { /* ignore */ }
  });
}

// eslint-disable-next-line no-control-regex
const ANSI_RE = /\x1b\[[0-9;]*m/g;

function parseRustcErrors(text) {
  const markers = [];
  const clean = text.replace(ANSI_RE, '');
  const re = /^(error|warning)(?:\[([A-Z]\d+)\])?: (.+)\n\s*--> [^:]+:(\d+):(\d+)/gm;
  let m;
  while ((m = re.exec(clean)) !== null) {
    const severity = m[1] === 'warning' ? monaco.MarkerSeverity.Warning : monaco.MarkerSeverity.Error;
    const code = m[2] || '';
    const message = m[3];
    const line = parseInt(m[4], 10);
    const col = parseInt(m[5], 10);
    const after = clean.substring(m.index + m[0].length, m.index + m[0].length + 500);
    const ul = after.match(/^\s*\|?\s*(\^+)/m);
    const endCol = ul ? col + ul[1].length : col + 1;
    markers.push({ severity, message: code ? `${code}: ${message}` : message,
      startLineNumber: line, startColumn: col, endLineNumber: line, endColumn: endCol,
      source: 'rustc', code });
  }
  return markers;
}

function escapeHtml(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function linkifyRustErrors(html) {
  return html.replace(
    /rustc --explain (E\d{4})/g,
    '<a href="https://doc.rust-lang.org/error_codes/$1.html" target="_blank" rel="noopener">rustc --explain $1</a>',
  );
}

class ConsoleOutput {
  constructor() {
    this._el = document.getElementById('console-content');
  }
  clear() { this._el.innerHTML = ''; }
  cmd(text) {
    const span = document.createElement('span');
    span.className = 'cmd';
    span.textContent = text;
    this._el.appendChild(span);
    this._el.appendChild(document.createTextNode('\n'));
  }
  out(text) {
    if (!text) return;
    const span = document.createElement('span');
    span.className = 'out';
    span.textContent = text;
    this._el.appendChild(span);
    this._el.appendChild(document.createTextNode('\n'));
  }
  err(text) {
    if (!text) return;
    const span = document.createElement('span');
    span.className = 'err';
    const clean = text.replace(ANSI_RE, '');
    span.innerHTML = linkifyRustErrors(escapeHtml(clean));
    this._el.appendChild(span);
    this._el.appendChild(document.createTextNode('\n'));
  }
  get text() { return this._el.textContent; }
}

async function onDOMContentLoaded() {
  const goEditorEl = document.getElementById('go-editor');
  const rustEditorEl = document.getElementById('rust-editor');
  const vmStatusEl = document.getElementById('vm-status');
  const vmLabelEl = vmStatusEl.querySelector('.vm-label');
  const vmOverlay = document.getElementById('vm-terminal-overlay');
  const vmCloseBtn = document.getElementById('vm-terminal-close');
  const vmOutputEl = document.getElementById('vm-terminal-output');
  const consoleSection = document.getElementById('console-section');
  const resizeHandle = document.getElementById('resize-handle');

  const go2rust = new Go2RustCompiler();
  const compiler = new RustRunner();
  const con = new ConsoleOutput();
  let pipelineGeneration = 0;

  // xterm.js
  const term = new Terminal({
    fontSize: 12,
    fontFamily: "'SF Mono', 'Cascadia Code', 'Fira Code', monospace",
    theme: { background: '#0d1117', foreground: '#c9d1d9' },
    convertEol: true, scrollback: 5000, cursorStyle: 'bar', cursorBlink: true,
  });
  const fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.open(vmOutputEl);
  term.onData((data) => { if (compiler._emulator) compiler._emulator.serial0_send(data); });

  let serialByteQueue = [];
  let serialFlushTimer = null;
  compiler.onSerialByte((byte) => {
    serialByteQueue.push(byte);
    if (!serialFlushTimer) {
      serialFlushTimer = setTimeout(() => {
        if (serialByteQueue.length > 0) term.write(new Uint8Array(serialByteQueue));
        serialByteQueue = [];
        serialFlushTimer = null;
      }, 50);
    }
  });

  // Resize
  const editorsEl = document.querySelector('.editors');
  resizeHandle.addEventListener('mousedown', (e) => {
    e.preventDefault();
    const startY = e.clientY;
    const startEH = editorsEl.offsetHeight;
    const startCH = consoleSection.offsetHeight;
    const total = startEH + startCH;
    editorsEl.style.flex = 'none';
    consoleSection.style.flex = 'none';
    editorsEl.style.height = startEH + 'px';
    consoleSection.style.height = startCH + 'px';
    function onMove(ev) {
      const h = Math.min(total - 200, Math.max(200, startEH + ev.clientY - startY));
      editorsEl.style.height = h + 'px';
      consoleSection.style.height = (total - h) + 'px';
    }
    function onUp() {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
    }
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  });

  // VM state
  compiler.onStateChange((state) => {
    vmStatusEl.dataset.state = state;
    vmLabelEl.textContent = stateLabel(state);
  });
  compiler.start().catch(() => {
    vmStatusEl.dataset.state = State.ERROR;
    vmLabelEl.textContent = 'VM error';
  });

  // VM popup
  vmStatusEl.addEventListener('click', () => {
    if (vmStatusEl.dataset.state !== 'ready') return;
    vmOverlay.classList.add('visible');
    fitAddon.fit();
    term.focus();
  });
  const resizeObserver = new ResizeObserver(() => {
    if (vmOverlay.classList.contains('visible')) fitAddon.fit();
  });
  resizeObserver.observe(vmOutputEl);
  function closeVmOverlay() { vmOverlay.classList.remove('visible'); }
  vmCloseBtn.addEventListener('click', closeVmOverlay);
  vmOverlay.addEventListener('click', (e) => { if (e.target === vmOverlay) closeVmOverlay(); });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && vmOverlay.classList.contains('visible')) closeVmOverlay();
  });

  // Shared editor config — identical for both, except readOnly
  const sharedOptions = {
    cursorSurroundingLines: 5, folding: false, fontSize: 13,
    glyphMargin: false, lineDecorationsWidth: 0, lineNumbers: 'on',
    lineNumbersMinChars: 3, minimap: { enabled: false },
    occurrencesHighlight: 'off', overviewRulerLanes: 0,
    renderFinalNewline: 'off', renderIndentGuides: false,
    renderLineHighlight: 'none', scrollBeyondLastLine: false,
    selectionHighlight: false, theme: 'vs-dark', automaticLayout: true,
    lightbulb: { enabled: 'off' }, quickSuggestions: false,
    contextmenu: false, hover: { enabled: true, delay: 200 },
  };
  const goEditor = monaco.editor.create(goEditorEl, { ...sharedOptions, language: 'go' });
  const goModel = goEditor.getModel();
  const rustEditor = monaco.editor.create(rustEditorEl, {
    ...sharedOptions, language: 'rust',
  });
  const rustModel = rustEditor.getModel();
  const highlightManager = new HighlightManager(goEditor, rustEditor);

  setupCopyButton('copy-output', () => rustModel.getValue());
  setupCopyButton('copy-console', () => con.text);

  // Explicit hover provider for Rust markers (Monaco's built-in marker hover
  // may be suppressed by the Rust language contribution from MonacoWebpackPlugin)
  monaco.languages.registerHoverProvider('rust', {
    provideHover(model, position) {
      const markers = monaco.editor.getModelMarkers({ resource: model.uri });
      for (const m of markers) {
        if (position.lineNumber >= m.startLineNumber && position.lineNumber <= m.endLineNumber &&
            position.column >= m.startColumn && position.column <= m.endColumn) {
          return {
            range: new monaco.Range(m.startLineNumber, m.startColumn, m.endLineNumber, m.endColumn),
            contents: [{ value: `**${m.source}(${m.code})**: ${m.message}` }],
          };
        }
      }
      return null;
    },
  });

  // Pipeline
  function onGoChanged() {
    const gen = ++pipelineGeneration;
    const goCode = goModel.getValue();

    // Reset
    rustModel.setValue('');
    monaco.editor.setModelMarkers(goModel, 'gors', []);
    monaco.editor.setModelMarkers(rustModel, 'rustc', []);
    highlightManager.setResult(null);
    con.clear();

    // Step 1: gors transpile
    con.cmd('$ gors build -o main.rs main.go');
    const goResult = go2rust.compile(goCode);

    if (!goResult.success) {
      const err = goResult.error;
      const loc = err.line > 0 ? `:${err.line}:${err.column}` : '';
      con.err(`main.go${loc}: ${err.kind}: ${err.message}`);

      if (err.line > 0) {
        const lines = goCode.split('\n');
        let endCol = err.endColumn || err.column + 1;
        if (err.line <= lines.length) endCol = Math.min(endCol, lines[err.line - 1].length + 1);
        monaco.editor.setModelMarkers(goModel, 'gors', [{
          severity: monaco.MarkerSeverity.Error, message: err.message,
          startLineNumber: err.line, startColumn: err.column,
          endLineNumber: err.line, endColumn: endCol,
          source: 'gors', code: err.kind,
        }]);
      }
      return;
    }

    const rustCode = goResult.rustCode;
    rustModel.setValue(rustCode);
    highlightManager.setResult(goResult.sourceMap);

    con.cmd('$ rustc --edition 2024 main.rs && ./main');
    compileAndRun(gen, rustCode);
  }

  async function compileAndRun(gen, rustCode) {
    if (compiler.state !== State.READY && compiler.state !== State.COMPILING) {
      await new Promise((resolve) => {
        const unsub = compiler.onStateChange((state) => {
          if (state === State.READY) { unsub(); resolve(); }
        });
      });
    }
    if (gen !== pipelineGeneration) return;

    const result = await compiler.compile(rustCode);
    if (gen !== pipelineGeneration) return;
    if (result.errors === 'cancelled') return;

    monaco.editor.setModelMarkers(rustModel, 'rustc', []);
    if (!result.success) {
      con.err(result.errors);
      monaco.editor.setModelMarkers(rustModel, 'rustc', parseRustcErrors(result.errors));
      return;
    }
    if (result.output) con.out(result.output);
    if (result.errors) con.err(result.errors);
  }

  goModel.onDidChangeContent(() => onGoChanged());

  goEditor.onMouseMove((e) => {
    if (e.target.position) highlightManager.highlightFromGo(e.target.position.lineNumber, e.target.position.column);
  });
  rustEditor.onMouseMove((e) => {
    if (e.target.position) highlightManager.highlightFromRust(e.target.position.lineNumber, e.target.position.column);
  });
  goEditor.onMouseLeave(() => highlightManager.clearRustHighlight());
  rustEditor.onMouseLeave(() => highlightManager.clearGoHighlight());
  // Make rust editor effectively read-only without using readOnly (which suppresses marker rendering)
  let rustExpectedValue = '';
  const origSetValue = rustModel.setValue.bind(rustModel);
  rustModel.setValue = (v) => { rustExpectedValue = v; origSetValue(v); };
  rustModel.onDidChangeContent(() => {
    const current = rustModel.getValue();
    if (current !== rustExpectedValue) {
      const markers = monaco.editor.getModelMarkers({ resource: rustModel.uri });
      origSetValue(rustExpectedValue);
      monaco.editor.setModelMarkers(rustModel, 'rustc', markers);
    }
  });

  goEditor.focus();
  goModel.setValue([
    'package main', '', 'import "fmt"', '',
    'func main() {', '\tfmt.Println("Hello, World!")', '}',
  ].join('\n'));
  goEditor.setPosition({ lineNumber: 6, column: 2 });
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', onDOMContentLoaded);
} else {
  onDOMContentLoaded();
}
