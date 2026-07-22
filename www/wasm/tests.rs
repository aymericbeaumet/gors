use gors::error::{Diagnostic, DiagnosticKind};

use crate::build_result::{BuildResult, extract_rust_token_at, utf16_column_to_byte_offset};
use crate::build_rust;

#[test]
fn browser_diagnostics_convert_go_byte_columns_to_utf16() {
    let diagnostic = Diagnostic::new("main.go", 1, 4, "bad rune", DiagnosticKind::Scanner)
        .with_source_line("é 😀name");
    assert_eq!(diagnostic.end_column, 8);

    let result = BuildResult::error_result(diagnostic);

    assert_eq!(result.error_column(), 3);
    assert_eq!(result.error_end_column(), 5);
}

#[test]
fn rust_token_lookup_uses_utf16_source_map_columns() {
    let source = "/* é😀 */ 𐐀name!();";
    let byte_offset = source.find("𐐀name").unwrap();
    let utf16_column = source[..byte_offset].encode_utf16().count() as u32;

    assert_eq!(
        extract_rust_token_at(source, 0, utf16_column).as_deref(),
        Some("𐐀name!")
    );
    assert_eq!(utf16_column_to_byte_offset("😀x", 2), Some(4));
    assert_eq!(utf16_column_to_byte_offset("😀x", 1), None);
}

#[test]
fn supported_bootstrap_build_returns_rust_and_explicit_source_map() {
    let input = r#"package main

func add(a int, b int) int {
	return a + b
}

func main() {
	total := 0
	for i := 0; i < 4; i = i + 1 {
		total = total + i
	}
	println(add(total, 2))
}
"#;

    let result = build_rust(input.to_string());

    assert!(result.success());
    assert!(result.output().contains("fn main()"));
    assert!(result.mapping_count() > 0);
    assert!(!result.get_mapping_positions().is_empty());
    assert_ne!(result.get_source_map_json(), "");
}

#[test]
fn supported_comments_are_preserved_without_legacy_statement_mapping() {
    let input = r#"package main

func main() {
	println(1)
	// preserved by the browser presentation layer
	println(2)
}
"#;

    let result = build_rust(input.to_string());

    assert!(result.success());
    assert_eq!(
        result
            .output()
            .matches("// preserved by the browser presentation layer")
            .count(),
        1
    );
}

#[test]
fn imports_return_a_structured_unsupported_diagnostic() {
    let input = r#"package main

import "fmt"

func main() {
	fmt.Println("hello")
}
"#;

    let result = build_rust(input.to_string());

    assert!(!result.success());
    assert_eq!(result.error_kind(), "compiler");
    assert_eq!(result.error_file(), "main.go");
    assert_eq!(result.error_line(), 1);
    assert!(result.error_message().contains("GORS2001"));
    assert!(
        result
            .error_message()
            .contains("imports are not implemented by the HIR/MIR backend")
    );
    assert_eq!(result.error_source_line(), "package main");
}

#[test]
fn semantic_errors_keep_the_backend_code_and_source_position() {
    let input = r#"package main

func main() {
	println(missing)
}
"#;

    let result = build_rust(input.to_string());

    assert!(!result.success());
    assert_eq!(result.error_kind(), "compiler");
    assert_eq!(result.error_file(), "main.go");
    assert_eq!(result.error_line(), 4);
    assert_eq!(result.error_column(), 10);
    assert!(result.error_message().contains("GORS2002"));
    assert!(
        result
            .error_message()
            .contains("undefined identifier missing")
    );
    assert_eq!(result.error_source_line(), "\tprintln(missing)");
}
