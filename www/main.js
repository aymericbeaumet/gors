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
  constructor(goEditor, rustEditor) {
    this.goEditor = goEditor;
    this.rustEditor = rustEditor;
    this.goDecorations = [];
    this.rustDecorations = [];
    this.rustResult = null;
  }

  setRustResult(result) {
    this.rustResult = result;
  }

  /**
   * Highlight Rust code corresponding to a Go position
   */
  highlightFromGo(line, column) {
    if (this.rustResult && this.rustResult.success) {
      const rustSpan = this.rustResult.go_to_output(line, column);
      if (rustSpan.length === 4) {
        this.rustDecorations = this.rustEditor.deltaDecorations(
          this.rustDecorations,
          [{
            range: new monaco.Range(rustSpan[0], rustSpan[1], rustSpan[2], rustSpan[3]),
            options: {
              className: 'source-map-highlight',
              isWholeLine: false,
            }
          }]
        );
      } else {
        this.clearRustHighlight();
      }
    }
  }

  /**
   * Highlight Go code corresponding to a Rust position
   */
  highlightFromRust(line, column) {
    if (!this.rustResult || !this.rustResult.success) {
      this.clearGoHighlight();
      return;
    }

    const goSpan = this.rustResult.output_to_go(line, column);
    if (goSpan.length === 4) {
      this.goDecorations = this.goEditor.deltaDecorations(
        this.goDecorations,
        [{
          range: new monaco.Range(goSpan[0], goSpan[1], goSpan[2], goSpan[3]),
          options: {
            className: 'source-map-highlight',
            isWholeLine: false,
          }
        }]
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

/**
 * Console manager for shell-style output
 */
class ConsoleManager {
  constructor(element) {
    this.element = element;
  }

  clear() {
    this.element.innerHTML = '';
  }

  addCommand(command) {
    const line = document.createElement('div');
    line.className = 'console-line';
    line.innerHTML = `<span class="prompt">$</span><span class="command">${escapeHtml(command)}</span>`;
    this.element.appendChild(line);
    this.scrollToBottom();
  }

  addOutput(text, className = '') {
    const output = document.createElement('div');
    output.className = `console-output ${className}`;
    output.textContent = text;
    this.element.appendChild(output);
    this.scrollToBottom();
  }

  addError(text) {
    this.addOutput(text, 'error');
  }

  addSuccess(text) {
    this.addOutput(text, 'success');
  }

  addErrorBlock(html) {
    const block = document.createElement('div');
    block.className = 'console-error-block';
    block.innerHTML = html;
    this.element.appendChild(block);
    this.scrollToBottom();
  }

  scrollToBottom() {
    this.element.scrollTop = this.element.scrollHeight;
  }
}

/**
 * Copy button handler
 */
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
      console.error('Failed to copy:', err);
    }
  });
}

async function onDOMContentLoaded() {
  const goEditorEl = document.getElementById('go-editor');
  const rustEditorEl = document.getElementById('rust-editor');
  const consoleEl = document.getElementById('console');

  const consoleManager = new ConsoleManager(consoleEl);

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

  // Create Go editor (editable)
  const goEditor = monaco.editor.create(goEditorEl, {
    ...editorOptions,
    language: 'go',
  });
  const goModel = goEditor.getModel();

  // Create Rust editor (read-only)
  const rustEditor = monaco.editor.create(rustEditorEl, {
    ...editorOptions,
    language: 'rust',
    readOnly: true,
    contextmenu: false,
    matchBrackets: 'never',
  });
  const rustModel = rustEditor.getModel();

  // Setup highlight manager
  const highlightManager = new HighlightManager(goEditor, rustEditor);

  // Setup copy button
  setupCopyButton('copy-rust', () => rustModel.getValue());

  // Build and update function
  function buildAndUpdate() {
    const code = goModel.getValue();

    // Clear previous markers
    monaco.editor.setModelMarkers(goModel, 'gors', []);
    consoleManager.clear();

    // Show build command
    consoleManager.addCommand('gors build --emit=rust main.go');

    // Build Rust
    const rustResult = gors.build_rust(code);
    highlightManager.setRustResult(rustResult);

    if (rustResult.success) {
      rustModel.setValue(rustResult.output);
      consoleManager.addSuccess('Build successful');
    } else {
      // Show error in console
      consoleManager.addError(`Rust compilation failed: ${rustResult.error_message}`);
      consoleManager.addErrorBlock(formatError(rustResult, code));

      // Add error markers
      const markers = createMonacoMarkers(rustResult, code);
      monaco.editor.setModelMarkers(goModel, 'gors', markers);
    }
  }

  // Register handlers
  goModel.onDidChangeContent(() => {
    buildAndUpdate();
  });

  // Hover handlers for source mapping
  goEditor.onMouseMove((e) => {
    if (e.target.position) {
      highlightManager.highlightFromGo(
        e.target.position.lineNumber,
        e.target.position.column
      );
    }
  });

  rustEditor.onMouseMove((e) => {
    if (e.target.position) {
      highlightManager.highlightFromRust(
        e.target.position.lineNumber,
        e.target.position.column
      );
    }
  });

  // Clear highlights when mouse leaves
  goEditor.onMouseLeave(() => {
    highlightManager.clearRustHighlight();
  });

  rustEditor.onMouseLeave(() => {
    highlightManager.clearGoHighlight();
  });

  // Prevent typing in read-only editor
  rustEditor.onKeyDown((event) => {
    if (!(event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      event.stopPropagation();
    }
  });

  // Initialization
  goEditor.focus();
  goModel.setValue([
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
  goEditor.setPosition({ lineNumber: 9, column: 2 });
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', onDOMContentLoaded);
} else {
  onDOMContentLoaded();
}
