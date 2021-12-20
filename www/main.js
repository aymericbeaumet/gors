import * as gors from 'gors';
import * as rustfmt from 'rustfmt';
import * as monaco from 'monaco-editor';

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
    try {
      const code = inputModel.getValue();
      const built = gors.build(code, rustfmt.format);
      outputModel.setValue(built);
      error.innerHTML = '';
    } catch (err) {
      error.innerHTML = err;
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
