import * as gors from 'gors';
import * as monaco from 'monaco-editor';

/**
 * Format an error with source context for display in the error panel
 */
function formatError(result, sourceCode) {
  const lines = sourceCode.split('\n');
  const errorLine = result.error_line;
  const errorColumn = result.error_column;
  const errorEndColumn = result.error_end_column || errorColumn + 1;
  const errorMessage = result.error_message;
  const errorKind = result.error_kind;

  // Build location string
  const location = errorLine > 0
    ? `<span class="error-location">main.go:${errorLine}:${errorColumn}</span>`
    : '<span class="error-location">main.go</span>';

  // Build error kind string
  const kindStr = errorKind === 'scanner' ? 'scanner error'
    : errorKind === 'parser' ? 'syntax error'
    : 'compile error';

  let output = `${location}: <span class="error-kind">${kindStr}</span>: <span class="error-message">${escapeHtml(errorMessage)}</span>\n`;

  // Add source context if we have line information
  if (errorLine > 0 && errorLine <= lines.length) {
    const sourceLine = result.error_source_line || lines[errorLine - 1];
    const lineNumStr = String(errorLine).padStart(4, ' ');

    output += `<span class="error-line-number">${lineNumStr} | </span><span class="error-source">${escapeHtml(sourceLine)}</span>\n`;

    // Add underline/caret pointing to error position
    const gutterWidth = lineNumStr.length + 3; // " | "
    const prefix = errorColumn > 1 ? ' '.repeat(errorColumn - 1) : '';
    const underlineLength = Math.max(1, errorEndColumn - errorColumn);
    const underline = underlineLength > 1 ? '~'.repeat(underlineLength) : '^';
    output += `${' '.repeat(gutterWidth)}${prefix}<span class="error-caret">${underline}</span>\n`;
  }

  return output;
}

/**
 * Escape HTML special characters
 */
function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

/**
 * Create Monaco editor markers from a build result
 */
function createMonacoMarkers(result, sourceCode) {
  if (result.success || result.error_line === 0) {
    return [];
  }

  const lines = sourceCode.split('\n');
  const lineCount = lines.length;
  const errorLine = result.error_line;
  const errorColumn = result.error_column;
  let errorEndColumn = result.error_end_column || errorColumn + 1;

  // Ensure end column doesn't exceed line length
  if (errorLine > 0 && errorLine <= lineCount) {
    const lineLength = lines[errorLine - 1].length;
    errorEndColumn = Math.min(errorEndColumn, lineLength + 1);
  }

  return [{
    severity: monaco.MarkerSeverity.Error,
    message: result.error_message,
    startLineNumber: errorLine,
    startColumn: errorColumn,
    endLineNumber: errorLine,
    endColumn: errorEndColumn,
    source: 'gors',
    code: result.error_kind,
  }];
}

/**
 * Highlight state management
 */
class HighlightManager {
  constructor(inputEditor, outputEditor) {
    this.inputEditor = inputEditor;
    this.outputEditor = outputEditor;
    this.inputDecorations = [];
    this.outputDecorations = [];
    this.currentResult = null;
  }

  setResult(result) {
    this.currentResult = result;
  }

  /**
   * Highlight Rust code corresponding to a Go position
   */
  highlightFromGo(line, column) {
    if (!this.currentResult || !this.currentResult.success) {
      this.clearOutputHighlight();
      return;
    }

    const rustSpan = this.currentResult.go_to_rust(line, column);
    if (rustSpan.length === 4) {
      this.outputDecorations = this.outputEditor.deltaDecorations(
        this.outputDecorations,
        [{
          range: new monaco.Range(rustSpan[0], rustSpan[1], rustSpan[2], rustSpan[3]),
          options: {
            className: 'source-map-highlight',
            isWholeLine: false,
          }
        }]
      );
    } else {
      this.clearOutputHighlight();
    }
  }

  /**
   * Highlight Go code corresponding to a Rust position
   */
  highlightFromRust(line, column) {
    if (!this.currentResult || !this.currentResult.success) {
      this.clearInputHighlight();
      return;
    }

    const goSpan = this.currentResult.rust_to_go(line, column);
    if (goSpan.length === 4) {
      this.inputDecorations = this.inputEditor.deltaDecorations(
        this.inputDecorations,
        [{
          range: new monaco.Range(goSpan[0], goSpan[1], goSpan[2], goSpan[3]),
          options: {
            className: 'source-map-highlight',
            isWholeLine: false,
          }
        }]
      );
    } else {
      this.clearInputHighlight();
    }
  }

  clearInputHighlight() {
    this.inputDecorations = this.inputEditor.deltaDecorations(this.inputDecorations, []);
  }

  clearOutputHighlight() {
    this.outputDecorations = this.outputEditor.deltaDecorations(this.outputDecorations, []);
  }

  clearAll() {
    this.clearInputHighlight();
    this.clearOutputHighlight();
  }
}

function onDOMContentLoaded() {
  const input = document.getElementById('input');
  const output = document.getElementById('output');
  const error = document.getElementById('error');
  const copyButton = document.getElementById('copy-button');

  const opts = {
    cursorSurroundingLines: 5,
    folding: false,
    fontSize: '13px',
    glyphMargin: false,
    lineDecorationsWidth: 0,
    lineNumbers: 'off',
    lineNumbersMinChars: 2,
    minimap: { enabled: false },
    occurrencesHighlight: 'off',
    overviewRulerLanes: 0,
    renderFinalNewline: 'off',
    renderIndentGuides: false,
    renderLineHighlight: 'none',
    scrollBeyondLastLine: false,
    selectionHighlight: false,
  };

  // setup input editor

  const inputEditor = monaco.editor.create(input, {
    ...opts,
    language: 'go',
  });
  const inputModel = inputEditor.getModel();

  // setup output editor

  const outputEditor = monaco.editor.create(output, {
    ...opts,
    language: 'rust',
    contextmenu: false,
    matchBrackets: 'never',
    readOnly: true,
  });
  const outputModel = outputEditor.getModel();

  // Setup highlight manager
  const highlightManager = new HighlightManager(inputEditor, outputEditor);

  // Store the current build result
  let currentResult = null;

  // register handlers

  inputModel.onDidChangeContent(() => {
    const code = inputModel.getValue();

    // Clear previous markers
    monaco.editor.setModelMarkers(inputModel, 'gors', []);

    const result = gors.build(code);
    currentResult = result;
    highlightManager.setResult(result);

    if (result.success) {
      outputModel.setValue(result.output);
      error.innerHTML = '';
    } else {
      // Keep the last successful Rust output (don't clear it)
      error.innerHTML = formatError(result, code);

      // Add error markers to Monaco editor
      const markers = createMonacoMarkers(result, code);
      monaco.editor.setModelMarkers(inputModel, 'gors', markers);
    }
  });

  // Hover handlers for source mapping
  inputEditor.onMouseMove((e) => {
    if (e.target.position) {
      highlightManager.highlightFromGo(
        e.target.position.lineNumber,
        e.target.position.column
      );
    }
  });

  outputEditor.onMouseMove((e) => {
    if (e.target.position) {
      highlightManager.highlightFromRust(
        e.target.position.lineNumber,
        e.target.position.column
      );
    }
  });

  // Clear highlights when mouse leaves
  inputEditor.onMouseLeave(() => {
    highlightManager.clearOutputHighlight();
  });

  outputEditor.onMouseLeave(() => {
    highlightManager.clearInputHighlight();
  });

  outputEditor.onKeyDown((event) => {
    if (!(event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      event.stopPropagation();
    }
  });

  // Copy button handler
  copyButton.addEventListener('click', async () => {
    const rustCode = outputModel.getValue();
    if (!rustCode) return;

    try {
      await navigator.clipboard.writeText(rustCode);
      copyButton.classList.add('copied');
      copyButton.querySelector('span').textContent = 'Copied!';

      setTimeout(() => {
        copyButton.classList.remove('copied');
        copyButton.querySelector('span').textContent = 'Copy';
      }, 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  });

  // initialization

  inputEditor.focus();
  inputModel.setValue([
    'package main',
    '',
    'import "fmt"',
    '',
    'func main() {',
    '\tfmt.Println("Hello, 世界")',
    '',
    '\t// Start typing and see the changes!',
    '\t',
    '}',
  ].join('\n'));
  inputEditor.setPosition({ lineNumber: 9, column: 2 });
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', onDOMContentLoaded);
} else {
  onDOMContentLoaded();
}
