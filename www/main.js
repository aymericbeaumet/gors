import * as monaco from 'monaco-editor';
import { Go2RustCompiler } from './go2rust-compiler.js';
import { Rust2WasmCompiler, State } from './rust2wasm-compiler.js';
import { WasmRunner } from './wasm-runner.js';

function stateLabel(state, progress) {
  switch (state) {
    case State.INITIALIZING: return 'Initializing...';
    case State.DOWNLOADING: return progress > 0 ? `Downloading VM ${progress}%` : 'Downloading VM...';
    case State.BOOTING: return progress > 0 ? `Booting VM ${progress}%` : 'Booting VM...';
    case State.READY: return 'VM Ready';
    case State.COMPILING: return 'Compiling...';
    case State.ERROR: return 'VM Error';
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

  setResult(result) {
    this.result = result;
  }

  highlightFromGo(line, column) {
    if (!this.result || !this.result.success) {
      this.clearRustHighlight();
      return;
    }

    const span = this.result.go_to_output(line, column);
    if (span.length === 4) {
      this.rustDecorations = this.rustEditor.deltaDecorations(
        this.rustDecorations,
        [{
          range: new monaco.Range(span[0], span[1], span[2], span[3]),
          options: { className: 'source-map-highlight', isWholeLine: false },
        }],
      );
    } else {
      this.clearRustHighlight();
    }
  }

  highlightFromRust(line, column) {
    if (!this.result || !this.result.success) {
      this.clearGoHighlight();
      return;
    }

    const span = this.result.output_to_go(line, column);
    if (span.length === 4) {
      this.goDecorations = this.goEditor.deltaDecorations(
        this.goDecorations,
        [{
          range: new monaco.Range(span[0], span[1], span[2], span[3]),
          options: { className: 'source-map-highlight', isWholeLine: false },
        }],
      );
    } else {
      this.clearGoHighlight();
    }
  }

  clearGoHighlight() {
    this.goDecorations = this.goEditor.deltaDecorations(this.goDecorations, []);
  }

  clearRustHighlight() {
    this.rustDecorations = this.rustEditor.deltaDecorations(this.rustDecorations, []);
  }
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
    } catch {
      // ignore
    }
  });
}

function showOutput(text, isError) {
  const el = document.getElementById('console-content');
  el.textContent = text;
  el.className = isError ? 'console-content error' : 'console-content';
}

async function onDOMContentLoaded() {
  const goEditorEl = document.getElementById('go-editor');
  const rustEditorEl = document.getElementById('rust-editor');
  const wasmEditorEl = document.getElementById('wasm-editor');
  const wasmContainer = document.getElementById('wasm-container');
  const statusEl = document.getElementById('status');
  const debugToggle = document.getElementById('debug-toggle');
  const vmStatusEl = document.getElementById('vm-status');
  const vmLabelEl = vmStatusEl.querySelector('.vm-label');
  const vmOverlay = document.getElementById('vm-terminal-overlay');
  const vmCloseBtn = document.getElementById('vm-terminal-close');
  const vmOutput = document.getElementById('vm-terminal-output');
  const vmSpinner = document.getElementById('vm-spinner');

  const consoleSection = document.getElementById('console-section');
  const resizeHandle = document.getElementById('resize-handle');

  const go2rust = new Go2RustCompiler();
  const rust2wasm = new Rust2WasmCompiler();
  const runner = new WasmRunner();

  let pipelineGeneration = 0;
  let serialLogInterval = null;

  // WASM debug panel — initial state set by inline script via html.show-wasm class
  let debugVisible = document.documentElement.classList.contains('show-wasm');
  if (debugVisible) wasmContainer.classList.remove('hidden');

  function setDebugVisible(v) {
    debugVisible = v;
    debugToggle.classList.toggle('active', debugVisible);
    wasmContainer.classList.toggle('hidden', !debugVisible);
    document.documentElement.classList.toggle('show-wasm', debugVisible);
    const p = new URLSearchParams(window.location.search);
    if (debugVisible) p.set('wasm', '1'); else p.delete('wasm');
    const qs = p.toString();
    history.replaceState(null, '', qs ? '?' + qs : window.location.pathname);
  }

  debugToggle.addEventListener('click', () => setDebugVisible(!debugVisible));

  // Console resize handle
  const editorsEl = document.querySelector('.editors');
  const MIN_EDITORS = 200;
  const MIN_CONSOLE = 200;
  resizeHandle.addEventListener('mousedown', (e) => {
    e.preventDefault();
    const startY = e.clientY;
    const startEditorsH = editorsEl.offsetHeight;
    const startConsoleH = consoleSection.offsetHeight;
    const total = startEditorsH + startConsoleH;
    editorsEl.style.flex = 'none';
    consoleSection.style.flex = 'none';
    editorsEl.style.height = startEditorsH + 'px';
    consoleSection.style.height = startConsoleH + 'px';
    function onMove(ev) {
      const delta = ev.clientY - startY;
      const clampedEditors = Math.min(total - MIN_CONSOLE, Math.max(MIN_EDITORS, startEditorsH + delta));
      editorsEl.style.height = clampedEditors + 'px';
      consoleSection.style.height = (total - clampedEditors) + 'px';
    }
    function onUp() {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
    }
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  });

  function updateSerialLog() {
    vmOutput.textContent = rust2wasm.serialLog.join('');
    vmOutput.scrollTop = vmOutput.scrollHeight;
  }

  // VM state indicator
  rust2wasm.onStateChange((state, progress) => {
    vmStatusEl.dataset.state = state;
    vmLabelEl.textContent = stateLabel(state, progress);

    if (state === State.READY) {
      vmSpinner.classList.add('ready');
      vmOverlay.classList.remove('visible');
      if (serialLogInterval) {
        clearInterval(serialLogInterval);
        serialLogInterval = null;
      }
    }
  });

  rust2wasm.start().catch(() => {
    vmStatusEl.dataset.state = State.ERROR;
    vmLabelEl.textContent = 'VM boot failed';
    vmSpinner.classList.add('ready');
  });

  // VM terminal overlay
  vmStatusEl.addEventListener('click', () => {
    updateSerialLog();
    vmOverlay.classList.add('visible');
    if (!serialLogInterval) {
      serialLogInterval = setInterval(updateSerialLog, 200);
    }
  });
  vmCloseBtn.addEventListener('click', () => {
    vmOverlay.classList.remove('visible');
    if (serialLogInterval && rust2wasm.state === State.READY) {
      clearInterval(serialLogInterval);
      serialLogInterval = null;
    }
  });
  vmOverlay.addEventListener('click', (e) => {
    if (e.target === vmOverlay) {
      vmOverlay.classList.remove('visible');
      if (serialLogInterval && rust2wasm.state === State.READY) {
        clearInterval(serialLogInterval);
        serialLogInterval = null;
      }
    }
  });

  // Editors
  const editorOptions = {
    cursorSurroundingLines: 5,
    folding: false,
    fontSize: 13,
    glyphMargin: false,
    lineDecorationsWidth: 0,
    lineNumbers: 'on',
    lineNumbersMinChars: 3,
    minimap: { enabled: false },
    occurrencesHighlight: 'off',
    overviewRulerLanes: 0,
    renderFinalNewline: 'off',
    renderIndentGuides: false,
    renderLineHighlight: 'none',
    scrollBeyondLastLine: false,
    selectionHighlight: false,
    theme: 'vs-dark',
    automaticLayout: true,
  };

  const goEditor = monaco.editor.create(goEditorEl, { ...editorOptions, language: 'go' });
  const goModel = goEditor.getModel();

  const rustEditor = monaco.editor.create(rustEditorEl, {
    ...editorOptions, language: 'rust', readOnly: true, contextmenu: false, matchBrackets: 'never',
  });
  const rustModel = rustEditor.getModel();

  const wasmEditor = monaco.editor.create(wasmEditorEl, {
    ...editorOptions, language: 'wat', readOnly: true, contextmenu: false, matchBrackets: 'never',
  });
  const wasmModel = wasmEditor.getModel();

  const highlightManager = new HighlightManager(goEditor, rustEditor);

  setupCopyButton('copy-output', () => rustModel.getValue());

  // The main reactive pipeline: Go change → Rust → WASM → Run
  function onGoChanged() {
    const gen = ++pipelineGeneration;
    const goCode = goModel.getValue();

    monaco.editor.setModelMarkers(goModel, 'gors', []);

    // Step 1: Go → Rust
    const goResult = go2rust.compile(goCode);

    if (!goResult.success) {
      const err = goResult.error;
      const loc = err.line > 0 ? `${err.line}:${err.column}` : '';
      rustModel.setValue(`// ${err.kind}${loc ? ' at ' + loc : ''}: ${err.message}`);
      statusEl.textContent = `${err.kind}: ${err.message}`;
      statusEl.className = 'status error';
      highlightManager.setResult(null);
      showOutput(`${err.kind}: ${err.message}`, true);

      if (err.line > 0) {
        const lines = goCode.split('\n');
        let endCol = err.endColumn || err.column + 1;
        if (err.line <= lines.length) {
          endCol = Math.min(endCol, lines[err.line - 1].length + 1);
        }
        monaco.editor.setModelMarkers(goModel, 'gors', [{
          severity: monaco.MarkerSeverity.Error,
          message: err.message,
          startLineNumber: err.line,
          startColumn: err.column,
          endLineNumber: err.line,
          endColumn: endCol,
          source: 'gors',
          code: err.kind,
        }]);
      }
      return;
    }

    // Step 2: Display Rust
    rustModel.setValue(goResult.rustCode);
    highlightManager.setResult(goResult.sourceMap);
    statusEl.textContent = '';
    statusEl.className = 'status ok';

    // Step 3: Rust → WASM → Run (async, cancellable)
    compileAndRun(gen, goResult.rustCode);
  }

  async function compileAndRun(gen, rustCode) {
    if (rust2wasm.state !== State.READY && rust2wasm.state !== State.COMPILING) {
      return;
    }

    const compileResult = await rust2wasm.compile(rustCode);

    if (gen !== pipelineGeneration) return;

    if (!compileResult.success) {
      if (compileResult.errors === 'cancelled') return;
      showOutput(compileResult.errors, true);
      wasmModel.setValue(`;; compilation failed\n;; ${compileResult.errors}`);
      return;
    }

    // Show WASM info in debug editor
    if (debugVisible) {
      wasmModel.setValue(`;; WASM binary: ${compileResult.wasmBytes.length} bytes`);
    }

    // Step 4: Run WASM
    const runResult = runner.run(compileResult.wasmBytes);

    if (gen !== pipelineGeneration) return;

    if (runResult.success) {
      showOutput(runResult.output || '(no output)', false);
    } else {
      showOutput(runResult.error, true);
    }
  }

  goModel.onDidChangeContent(() => onGoChanged());

  // Source-map highlighting
  goEditor.onMouseMove((e) => {
    if (e.target.position) {
      highlightManager.highlightFromGo(e.target.position.lineNumber, e.target.position.column);
    }
  });
  rustEditor.onMouseMove((e) => {
    if (e.target.position) {
      highlightManager.highlightFromRust(e.target.position.lineNumber, e.target.position.column);
    }
  });
  goEditor.onMouseLeave(() => highlightManager.clearRustHighlight());
  rustEditor.onMouseLeave(() => highlightManager.clearGoHighlight());

  rustEditor.onKeyDown((event) => {
    if (!(event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      event.stopPropagation();
    }
  });

  // Initial code
  goEditor.focus();
  goModel.setValue([
    'package main',
    '',
    'import "fmt"',
    '',
    'func main() {',
    '\tfmt.Println("Hello, World!")',
    '}',
  ].join('\n'));
  goEditor.setPosition({ lineNumber: 6, column: 2 });
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', onDOMContentLoaded);
} else {
  onDOMContentLoaded();
}
