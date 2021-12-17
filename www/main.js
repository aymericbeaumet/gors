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
    minimap: {
      enabled: false
    },
    renderFinalNewline: false,
  };

  const inputEditor = monaco.editor.create(input, {
    language: 'go',
    value: `// You can edit this code!
// Click here and start typing.
package main

import "fmt"

func main() {
	fmt.Println("Hello, 世界")
}`,
    ...opts,
  });
  const inputModel = inputEditor.getModel();

  const outputEditor = monaco.editor.create(output, {
    language: 'rust',
    readOnly: true,
    ...opts,
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
