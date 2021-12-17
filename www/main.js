import * as gors from 'gors';
import * as rustfmt from 'rustfmt';
import hljs from 'highlight.js/lib/core';
import hljsRust from 'highlight.js/lib/languages/rust'
import throttle from 'lodash/throttle';
import 'highlight.js/styles/tomorrow-night-bright.css';

hljs.registerLanguage('rust', hljsRust);

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', onDOMContentLoaded);
} else {
  onDOMContentLoaded();
}

function onDOMContentLoaded() {
  const input = document.getElementById("input");
  const output = document.getElementById("output");
  const error = document.getElementById("error");
  input.addEventListener('input', throttle(onInput, 100), false);
  onInput();

  function onInput() {
    try {
      const compiled = gors.compile(input.value);
      const formatted = rustfmt.format(compiled);
      console.log(formatted);
      const hightlighted = hljs.highlight(formatted, {language: 'rust'}).value;
      output.innerHTML = hightlighted;
      error.innerHTML = '';
    } catch (err) {
      error.innerHTML = err;
    }
  }
}
