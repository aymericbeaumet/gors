use gors::error::{Diagnostic, DiagnosticKind};
use gors::sourcemap::SourceMap;

use crate::GorsCompiler;
use crate::build_result::{BuildResult, extract_rust_token_at, utf16_column_to_byte_offset};

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
fn supported_build_returns_rust_and_explicit_source_map() {
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

    let result = GorsCompiler::new().build_rust(input.to_string());

    assert!(result.success());
    assert!(result.output().contains("fn main()"));
    assert!(result.mapping_count() > 0);
    assert!(!result.get_mapping_positions().is_empty());
    assert_ne!(result.get_source_map_json(), "");
    assert_eq!(result.runtime_dependency_schema_version(), 1);
    assert_eq!(
        result.runtime_contract_identity(),
        gors::compiler::db::RuntimeAbiId::current().to_string()
    );
    assert_eq!(result.get_runtime_operation_ids(), [14, 16]);
    assert!(result.output().contains("::__gors_runtime::"));
    assert!(!result.output().contains("mod __gors_runtime"));
}

#[test]
fn runtime_dependency_is_unconditional_and_target_neutral() {
    let result = GorsCompiler::new().build_rust("package main\nfunc main() {}\n".to_string());

    assert!(result.success());
    assert_eq!(result.runtime_dependency_schema_version(), 1);
    assert_eq!(
        result.runtime_contract_identity(),
        gors::compiler::db::RuntimeAbiId::current().to_string()
    );
    assert!(result.get_runtime_operation_ids().is_empty());
    assert!(!result.output().contains("mod __gors_runtime"));
    assert!(!result.output().contains("runtime artifact"));
}

#[test]
fn supported_comments_are_preserved_through_semantic_lowering() {
    let input = r#"package main

func main() {
	println(1)
	// preserved by the browser presentation layer
	println(2)
}
"#;

    let result = GorsCompiler::new().build_rust(input.to_string());

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
fn multiline_block_comment_shifts_downstream_function_mappings_by_physical_lines() {
    let baseline = r#"package main

func first() int {
	println(1)
	return 1
}

func downstream() int {
	return 2
}

func main() {
	println(first() + downstream())
}
"#;
    let commented = r#"package main

func first() int {
	println(1)
	/* first block line
	second block line
	third block line */
	return 1
}

func downstream() int {
	return 2
}

func main() {
	println(first() + downstream())
}
"#;

    let baseline = GorsCompiler::new().build_rust(baseline.to_string());
    let commented = GorsCompiler::new().build_rust(commented.to_string());

    assert!(baseline.success());
    assert!(commented.success());
    assert_eq!(
        last_destination_line(&commented, "downstream"),
        last_destination_line(&baseline, "downstream") + 3
    );
    assert_eq!(
        commented
            .output()
            .matches("/* first block line\n\tsecond block line\n\tthird block line */")
            .count(),
        1
    );
}

#[test]
fn leading_comment_separator_shifts_downstream_function_mappings() {
    let padding = "\n".repeat(21);
    let program = |prefix: &str| {
        format!(
            "{prefix}package main\n{padding}func downstream() int {{\n\treturn 2\n}}\n\nfunc main() {{\n\tprintln(downstream())\n}}\n"
        )
    };

    let baseline = GorsCompiler::new().build_rust(program(""));
    let commented = GorsCompiler::new().build_rust(program("// leading browser comment\n"));

    assert!(baseline.success());
    assert!(commented.success());
    assert!(
        commented
            .output()
            .starts_with("// leading browser comment\n\n")
    );
    assert_eq!(
        last_destination_line(&commented, "downstream"),
        last_destination_line(&baseline, "downstream") + 2
    );
}

#[test]
fn imports_without_a_browser_catalog_return_a_structured_diagnostic() {
    let input = r#"package main

import "fmt"

func main() {
	fmt.Println("hello")
}
"#;

    let result = GorsCompiler::new().build_rust(input.to_string());

    assert!(!result.success());
    assert_eq!(result.error_kind(), "compiler");
    assert_eq!(result.error_file(), "main.go");
    assert_eq!(result.error_line(), 3);
    assert!(result.error_message().contains("GORS2004"));
    assert!(
        result
            .error_message()
            .contains("unresolved import \"fmt\": no package catalog owns this canonical path")
    );
    assert_eq!(result.error_source_line(), "import \"fmt\"");
    assert_eq!(result.runtime_dependency_schema_version(), 0);
    assert_eq!(result.runtime_contract_identity(), "");
    assert!(result.get_runtime_operation_ids().is_empty());
}

#[test]
fn semantic_errors_keep_the_backend_code_and_source_position() {
    let input = r#"package main

func main() {
	println(missing)
}
"#;

    let result = GorsCompiler::new().build_rust(input.to_string());

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

#[test]
fn retained_browser_session_reuses_an_unchanged_function_across_edits() {
    use gors::compiler::db::QueryKind;

    let initial = r#"package main

func stable() int {
	return 40
}

func main() {
	println(stable())
}
"#;
    let changed = r#"package main

func stable() int {
	return 40
}

func main() {
	println(stable() + 2)
}
"#;
    let mut compiler = GorsCompiler::new();

    assert!(compiler.build_rust(initial.to_string()).success());
    compiler.session().database().reset_telemetry();
    assert!(compiler.build_rust(changed.to_string()).success());

    let telemetry = compiler.session().database().telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 1);
    assert_eq!(telemetry.executions(QueryKind::NormalizedGoMir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 1);
}

#[test]
fn retained_browser_session_does_not_leak_comments_or_source_map_provenance() {
    let initial = r#"package main

func main() {
	// first revision
	println(1)
}
"#;
    let changed = r#"package main

func main() {
	// second revision has a longer comment
	println(2)
}
"#;
    let mut compiler = GorsCompiler::new();

    assert!(compiler.build_rust(initial.to_string()).success());
    let result = compiler.build_rust(changed.to_string());

    assert!(result.success());
    assert!(
        result
            .output()
            .contains("// second revision has a longer comment")
    );
    assert!(!result.output().contains("// first revision"));
    let source_map = result.get_source_map_json();
    assert!(source_map.contains("second revision has a longer comment"));
    assert!(!source_map.contains("first revision"));
}

#[test]
fn retained_browser_session_recovers_after_a_syntax_invalid_revision() {
    let mut compiler = GorsCompiler::new();

    let invalid = compiler.build_rust("package main\nfunc main( {\n".to_string());
    assert!(!invalid.success());

    let recovered =
        compiler.build_rust("package main\n\nfunc main() {\n\tprintln(1)\n}\n".to_string());
    assert!(recovered.success());
}

fn last_destination_line(result: &BuildResult, name: &str) -> u32 {
    let json = result.get_source_map_json();
    let source_map = SourceMap::from_reader(json.as_bytes()).unwrap();
    source_map
        .tokens()
        .filter(|token| token.get_name() == Some(name))
        .map(|token| token.get_dst_line())
        .max()
        .unwrap()
}
