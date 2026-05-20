export class WasmRunner {
  run(wasmBytes) {
    const output = [];
    let instance;

    const importObject = {
      env: {
        print_i32: (value) => {
          output.push(String(value));
        },
        print_str: (ptr, len) => {
          if (!instance) return;
          const memory = instance.exports.memory;
          const bytes = new Uint8Array(memory.buffer, ptr, len);
          output.push(new TextDecoder().decode(bytes));
        },
      },
    };

    try {
      const module = new WebAssembly.Module(wasmBytes);
      instance = new WebAssembly.Instance(module, importObject);
    } catch (err) {
      return { success: false, output: '', error: `WASM instantiation failed: ${err.message}` };
    }

    try {
      if (typeof instance.exports.main === 'function') {
        instance.exports.main();
      } else if (typeof instance.exports._start === 'function') {
        instance.exports._start();
      } else {
        return { success: false, output: '', error: 'No main or _start function found' };
      }
    } catch (err) {
      return { success: false, output: output.join(''), error: `Runtime error: ${err.message}` };
    }

    return { success: true, output: output.join(''), error: '' };
  }
}
