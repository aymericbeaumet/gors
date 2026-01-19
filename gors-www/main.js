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

function onDOMContentLoaded() {
  const input = document.getElementById('input');
  const output = document.getElementById('output');
  const error = document.getElementById('error');

  const opts = {
    cursorSurroundingLines: 5,
    folding: false,
    fontSize: '13px',
    glyphMargin: false,
    lineDecorationsWidth: 0,
    lineNumbers: 'off',
    lineNumbersMinChars: 2,
    minimap: { enabled: false },
    occurrencesHighlight: false,
    overviewRulerLanes: 0,
    renderFinalNewline: false,
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

  // register handlers

  inputModel.onDidChangeContent(() => {
    const code = inputModel.getValue();

    // Clear previous markers
    monaco.editor.setModelMarkers(inputModel, 'gors', []);

    const result = gors.build(code);

    if (result.success) {
      outputModel.setValue(result.output);
      error.innerHTML = '';
    } else {
      outputModel.setValue('');
      error.innerHTML = formatError(result, code);

      // Add error markers to Monaco editor
      const markers = createMonacoMarkers(result, code);
      monaco.editor.setModelMarkers(inputModel, 'gors', markers);
    }
  });

  outputEditor.onKeyDown((event) => {
    if (!(event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      event.stopPropagation();
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
