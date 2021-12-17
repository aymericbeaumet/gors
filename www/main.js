import * as gors from 'gors';
import * as rustfmt from 'rustfmt';
import * as monaco from 'monaco-editor';

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', onDOMContentLoaded);
} else {
  onDOMContentLoaded();
}

function onDOMContentLoaded() {
  const input = document.getElementById("input");
  const output = document.getElementById("output");
  const error = document.getElementById("error");

  const opts = {
    fontSize: "13px",
    minimap: {enabled: false},
    renderFinalNewline: false,
    renderLineHighlight: "none",
    // https://stackoverflow.com/a/53448744/1071486
    lineNumbers: 'off',
    glyphMargin: false,
    folding: false,
    lineDecorationsWidth: 0,
    lineNumbersMinChars: 0,
  };

  const inputEditor = monaco.editor.create(input, {
    ...opts,
    language: 'go',
    value: `// You can edit this code!
// Click here and start typing.
package main

import "fmt"

func main() {
	fmt.Println("Hello, 世界")
}`,
  });
  const inputModel = inputEditor.getModel();

  const outputEditor = monaco.editor.create(output, {
    ...opts,
    language: 'rust',
    readOnly: true,
  });
  const outputModel = outputEditor.getModel();

  inputModel.onDidChangeContent(onChange)
  onChange()

  function onChange() {
    try {
      const code = inputModel.getValue();
      const compiled = gors.compile(code);
      const formatted = rustfmt.format(compiled);
      outputModel.setValue(formatted);
      error.innerHTML = '';
    } catch (err) {
      error.innerHTML = err;
    }
  }
}
