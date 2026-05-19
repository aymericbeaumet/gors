import * as gors from 'gors';
import * as monaco from 'monaco-editor';

function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
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

  clearAll() {
    this.clearGoHighlight();
    this.clearRustHighlight();
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
    } catch (err) {
      // ignore
    }
  });
}

async function onDOMContentLoaded() {
  const goEditorEl = document.getElementById('go-editor');
  const rustEditorEl = document.getElementById('rust-editor');
  const statusEl = document.getElementById('status');

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

  const goEditor = monaco.editor.create(goEditorEl, {
    ...editorOptions,
    language: 'go',
  });
  const goModel = goEditor.getModel();

  const rustEditor = monaco.editor.create(rustEditorEl, {
    ...editorOptions,
    language: 'rust',
    readOnly: true,
    contextmenu: false,
    matchBrackets: 'never',
  });
  const rustModel = rustEditor.getModel();

  const highlightManager = new HighlightManager(goEditor, rustEditor);

  setupCopyButton('copy-output', () => rustModel.getValue());

  function transpile() {
    const code = goModel.getValue();

    monaco.editor.setModelMarkers(goModel, 'gors', []);

    const result = gors.build_rust(code);
    highlightManager.setResult(result);

    if (result.success) {
      rustModel.setValue(result.output);
      statusEl.textContent = '';
      statusEl.className = 'status ok';
    } else {
      const kind = result.error_kind === 'scanner' ? 'scanner error'
        : result.error_kind === 'parser' ? 'syntax error'
          : 'compile error';

      const loc = result.error_line > 0
        ? `${result.error_line}:${result.error_column}`
        : '';

      rustModel.setValue(`// ${kind}${loc ? ' at ' + loc : ''}: ${result.error_message}`);
      statusEl.textContent = `${kind}: ${result.error_message}`;
      statusEl.className = 'status error';

      if (result.error_line > 0) {
        const lines = code.split('\n');
        let endCol = result.error_end_column || result.error_column + 1;
        if (result.error_line <= lines.length) {
          endCol = Math.min(endCol, lines[result.error_line - 1].length + 1);
        }

        monaco.editor.setModelMarkers(goModel, 'gors', [{
          severity: monaco.MarkerSeverity.Error,
          message: result.error_message,
          startLineNumber: result.error_line,
          startColumn: result.error_column,
          endLineNumber: result.error_line,
          endColumn: endCol,
          source: 'gors',
          code: result.error_kind,
        }]);
      }
    }
  }

  goModel.onDidChangeContent(() => transpile());

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
