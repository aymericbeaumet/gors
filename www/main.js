import * as gors from 'gors';
import { throttle } from 'lodash';

const input = document.getElementById("input");
const output = document.getElementById("output");
const error = document.getElementById("error");

compile();
input.addEventListener('input', throttle(compile, 100), false);

function compile() {
  try {
    output.value = gors.compile(input.value);
    error.innerHTML = '';
  } catch (err) {
    error.innerHTML = err;
  }
}
