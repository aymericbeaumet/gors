import * as gors from 'gors';

export class Go2RustCompiler {
  compile(goSource) {
    const result = gors.build_rust(goSource);

    if (result.success) {
      return {
        success: true,
        rustCode: result.output,
        sourceMap: result,
        error: null,
      };
    }

    const kind = result.error_kind === 'scanner' ? 'scanner error'
      : result.error_kind === 'parser' ? 'syntax error'
        : 'compile error';

    return {
      success: false,
      rustCode: '',
      sourceMap: null,
      error: {
        message: result.error_message,
        kind,
        line: result.error_line,
        column: result.error_column,
        endColumn: result.error_end_column,
      },
    };
  }
}
