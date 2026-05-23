import * as gors from 'gors-wasm';

const MAX_CACHE_ENTRIES = 32;

export class Go2RustCompiler {
  constructor() {
    this.cache = new Map();
  }

  compile(goSource) {
    if (this.cache.has(goSource)) {
      const cached = this.cache.get(goSource);
      this.cache.delete(goSource);
      this.cache.set(goSource, cached);
      return cached;
    }

    const result = gors.build_rust(goSource);

    const normalized = result.success
      ? {
        success: true,
        rustCode: result.output,
        sourceMap: result,
        error: null,
      }
      : {
      success: false,
      rustCode: '',
      sourceMap: null,
      error: {
        message: result.error_message,
        kind: result.error_kind === 'scanner' ? 'scanner error'
          : result.error_kind === 'parser' ? 'syntax error'
            : 'compile error',
        line: result.error_line,
        column: result.error_column,
        endColumn: result.error_end_column,
      },
    };

    this.cache.set(goSource, normalized);
    if (this.cache.size > MAX_CACHE_ENTRIES) {
      const oldest = this.cache.keys().next().value;
      this.cache.delete(oldest);
    }

    return normalized;
  }
}
