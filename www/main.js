import * as gors from 'gors';
import * as monaco from 'monaco-editor';

// Register WAT (WebAssembly Text) language for syntax highlighting
monaco.languages.register({ id: 'wat' });

monaco.languages.setMonarchTokensProvider('wat', {
  keywords: [
    'module', 'type', 'func', 'param', 'result', 'local', 'global',
    'table', 'memory', 'elem', 'data', 'start', 'import', 'export',
    'mut', 'offset', 'align', 'if', 'then', 'else', 'end', 'loop',
    'block', 'br', 'br_if', 'br_table', 'return', 'call', 'call_indirect',
    'drop', 'select', 'unreachable', 'nop'
  ],
  typeKeywords: [
    'i32', 'i64', 'f32', 'f64', 'v128', 'funcref', 'externref', 'anyfunc'
  ],
  operators: [],
  symbols: /[=><!~?:&|+\-*\/\^%]+/,

  tokenizer: {
    root: [
      // Comments
      [/;;.*$/, 'comment'],
      [/\(;/, 'comment', '@comment'],

      // Strings
      [/"([^"\\]|\\.)*$/, 'string.invalid'],
      [/"/, 'string', '@string'],

      // Numbers
      [/-?0x[0-9a-fA-F_]+/, 'number.hex'],
      [/-?\d+(\.\d+)?([eE][+-]?\d+)?/, 'number'],

      // Identifiers and keywords
      [/\$[a-zA-Z_][a-zA-Z0-9_]*/, 'variable'],
      [/[a-z_][a-z0-9_.]*/, {
        cases: {
          '@keywords': 'keyword',
          '@typeKeywords': 'type',
          '@default': 'identifier'
        }
      }],

      // Instructions (i32.add, f64.mul, etc.)
      [/(i32|i64|f32|f64|v128)\.[a-z_][a-z0-9_]*/, 'keyword.instruction'],
      [/(memory|table|global|local)\.(get|set|tee|size|grow|copy|fill)/, 'keyword.instruction'],

      // Parentheses
      [/[()]/, 'delimiter.parenthesis'],

      // Whitespace
      [/\s+/, 'white'],
    ],

    comment: [
      [/[^;)]+/, 'comment'],
      [/;\)/, 'comment', '@pop'],
      [/[;)]/, 'comment'],
    ],

    string: [
      [/[^\\"]+/, 'string'],
      [/\\./, 'string.escape'],
      [/"/, 'string', '@pop'],
    ],
  },
});

// Configure WAT language settings
monaco.languages.setLanguageConfiguration('wat', {
  comments: {
    lineComment: ';;',
    blockComment: ['(;', ';)'],
  },
  brackets: [['(', ')']],
  autoClosingPairs: [
    { open: '(', close: ')' },
    { open: '"', close: '"' },
  ],
});

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

  // Build error kind string
  const kindStr = errorKind === 'scanner' ? 'scanner error'
    : errorKind === 'parser' ? 'syntax error'
    : 'compile error';

  // Build location and message on one line
  const location = errorLine > 0 ? `main.go:${errorLine}:${errorColumn}` : 'main.go';
  let output = `<span class="error-location">${location}</span>: <span class="error-kind">${kindStr}</span>: <span class="error-message">${escapeHtml(errorMessage)}</span>\n`;

  // Add source context if we have line information
  if (errorLine > 0 && errorLine <= lines.length) {
    const sourceLine = result.error_source_line || lines[errorLine - 1];
    const lineNumStr = String(errorLine).padStart(4, ' ');

    output += `<span class="error-line-number">${lineNumStr} | </span><span class="error-source">${escapeHtml(sourceLine)}</span>\n`;

    // Add caret pointing to error position
    const gutterWidth = lineNumStr.length + 3; // " | "
    const prefix = errorColumn > 1 ? ' '.repeat(errorColumn - 1) : '';
    const underlineLength = Math.max(1, errorEndColumn - errorColumn);
    const caret = underlineLength > 1 ? '~'.repeat(underlineLength) : '^';
    output += `${' '.repeat(gutterWidth)}${prefix}<span class="error-caret">${caret}</span>`;
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
  constructor(goEditor, outputEditor) {
    this.goEditor = goEditor;
    this.outputEditor = outputEditor;
    this.goDecorations = [];
    this.outputDecorations = [];
    this.rustResult = null;
    this.outputMode = 'rust'; // 'rust' or 'wasm'
  }

  setRustResult(result) {
    this.rustResult = result;
  }

  setOutputMode(mode) {
    this.outputMode = mode;
  }

  /**
   * Highlight output code corresponding to a Go position
   */
  highlightFromGo(line, column) {
    // Only highlight when in Rust mode (WASM doesn't have source mapping)
    if (this.outputMode !== 'rust') {
      this.clearOutputHighlight();
      return;
    }

    if (this.rustResult && this.rustResult.success) {
      const outputSpan = this.rustResult.go_to_output(line, column);
      if (outputSpan.length === 4) {
        this.outputDecorations = this.outputEditor.deltaDecorations(
          this.outputDecorations,
          [{
            range: new monaco.Range(outputSpan[0], outputSpan[1], outputSpan[2], outputSpan[3]),
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
  }

  /**
   * Highlight Go code corresponding to an output position
   */
  highlightFromOutput(line, column) {
    // Only highlight when in Rust mode (WASM doesn't have source mapping)
    if (this.outputMode !== 'rust') {
      this.clearGoHighlight();
      return;
    }

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

  clearOutputHighlight() {
    this.outputDecorations = this.outputEditor.deltaDecorations(this.outputDecorations, []);
  }

  clearAll() {
    this.clearGoHighlight();
    this.clearOutputHighlight();
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

  addFormattedError(html) {
    const block = document.createElement('div');
    block.className = 'console-formatted-error';
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
  const outputEditorEl = document.getElementById('output-editor');
  const consoleEl = document.getElementById('console');
  const outputContainer = document.getElementById('output-container');
  const outputToggle = document.getElementById('output-toggle');

  const consoleManager = new ConsoleManager(consoleEl);

  // Output mode state
  let outputMode = 'rust'; // 'rust' or 'wasm'
  let lastRustResult = null;
  let lastWasmResult = null;

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

  // Create output editor (read-only) - starts as Rust
  const outputEditor = monaco.editor.create(outputEditorEl, {
    ...editorOptions,
    language: 'rust',
    readOnly: true,
    contextmenu: false,
    matchBrackets: 'never',
  });
  const outputModel = outputEditor.getModel();

  // Setup highlight manager
  const highlightManager = new HighlightManager(goEditor, outputEditor);

  // Setup copy button
  setupCopyButton('copy-output', () => outputModel.getValue());

  // Update the output editor content based on current mode
  function updateOutputDisplay() {
    if (outputMode === 'rust') {
      monaco.editor.setModelLanguage(outputModel, 'rust');
      outputContainer.classList.remove('wasm');
      outputContainer.classList.add('rust');
      outputToggle.classList.remove('wasm');
      outputToggle.classList.add('rust');
      
      if (lastRustResult && lastRustResult.success) {
        outputModel.setValue(lastRustResult.output);
      } else if (lastRustResult) {
        outputModel.setValue('// Build failed - see console for errors');
      } else {
        outputModel.setValue('');
      }
    } else {
      // Use 'wat' language for WAT syntax highlighting
      monaco.editor.setModelLanguage(outputModel, 'wat');
      outputContainer.classList.remove('rust');
      outputContainer.classList.add('wasm');
      outputToggle.classList.remove('rust');
      outputToggle.classList.add('wasm');
      
      if (lastWasmResult && lastWasmResult.success) {
        outputModel.setValue(lastWasmResult.wat);
      } else if (lastWasmResult) {
        outputModel.setValue(';; Build failed: ' + lastWasmResult.error_message);
      } else if (lastRustResult && !lastRustResult.success) {
        outputModel.setValue(';; Build failed - see console for errors');
      } else {
        outputModel.setValue('');
      }
    }
    
    highlightManager.setOutputMode(outputMode);
  }

  // Toggle click handler - switches between modes
  outputToggle.addEventListener('click', () => {
    outputMode = outputMode === 'rust' ? 'wasm' : 'rust';
    updateOutputDisplay();
  });

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
    lastRustResult = rustResult;
    highlightManager.setRustResult(rustResult);

    if (rustResult.success) {
      consoleManager.addSuccess(`Rust compiled (${rustResult.output.length} bytes)`);

      // Build WASM
      consoleManager.addCommand('gors build --emit=wasm main.go');
      const wasmResult = gors.compile_to_wasm(code);
      lastWasmResult = wasmResult;

      if (wasmResult.success) {
        consoleManager.addSuccess(`WASM compiled (${wasmResult.wasm_bytes.length} bytes)`);

        // Run the code (only if WASM build succeeded)
        consoleManager.addCommand('gors run main.go');
        const runResult = gors.run_go(code);

        if (runResult.success) {
          if (runResult.output) {
            consoleManager.addOutput(runResult.output);
          } else {
            consoleManager.addOutput('(no output)');
          }
        } else {
          consoleManager.addError(runResult.error_message);
        }
      } else {
        consoleManager.addError(wasmResult.error_message);
      }
    } else {
      lastWasmResult = null;
      
      // Show error in console
      consoleManager.addFormattedError(formatError(rustResult, code));

      // Add error markers
      const markers = createMonacoMarkers(rustResult, code);
      monaco.editor.setModelMarkers(goModel, 'gors', markers);
    }

    // Update the output display
    updateOutputDisplay();
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

  outputEditor.onMouseMove((e) => {
    if (e.target.position) {
      highlightManager.highlightFromOutput(
        e.target.position.lineNumber,
        e.target.position.column
      );
    }
  });

  // Clear highlights when mouse leaves
  goEditor.onMouseLeave(() => {
    highlightManager.clearOutputHighlight();
  });

  outputEditor.onMouseLeave(() => {
    highlightManager.clearGoHighlight();
  });

  // Prevent typing in read-only editor
  outputEditor.onKeyDown((event) => {
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
