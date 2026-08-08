use super::*;

#[test]
fn def_id_function_names_cannot_collide_with_rust_keywords() {
    let run = compile_and_run(
        r#"
            package main
            func async() int { return 1 }
            func async_() int { return async() + 1 }
            func gen() int { return async_() + 1 }
            func main() { println(gen()) }
        "#,
    );

    assert_eq!(run.stderr, b"3\n");
    let stable_symbols = run
        .rust
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub fn __gors_fn_"))
        .filter_map(|suffix| suffix.split_once('(').map(|(symbol, _)| symbol))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(stable_symbols.len(), 3, "{}", run.rust);
    assert!(
        stable_symbols
            .iter()
            .all(|symbol| symbol.len() == 64 && symbol.bytes().all(|byte| byte.is_ascii_hexdigit())),
        "{}",
        run.rust
    );
    assert!(!run.rust.contains("fn async"));
    assert!(!run.rust.contains("fn gen"));
}

#[test]
fn basic_program_uses_the_hir_mir_pipeline() {
    let source = "package main\nfunc main() { x := 40 + 2; println(x) }\n";
    let rust = crate::printer::generate(compile_file("main.go", source).unwrap()).unwrap();
    assert!(rust.contains("fn main"), "{rust}");
    assert!(rust.contains("42"), "{rust}");
}

#[test]
fn unresolved_package_selectors_fail_before_codegen() {
    let source = "package main\nimport \"fmt\"\nfunc main() { fmt.Println(1) }\n";
    let result = compile_file("main.go", source);
    assert!(
        result.is_err(),
        "unresolved imports must be a semantic diagnostic"
    );
    let errors = result.err().unwrap();
    assert!(
        errors.iter().any(|error| error
            .message
            .contains("undefined package function fmt.Println")),
        "{errors:?}"
    );
}

#[test]
fn program_boundary_compiles_multiple_package_files() {
    let package = input::PackageKey::command_line();
    let manifest = input::PackageInputManifest::new(
        package,
        [
            input::SourceFileInput::from_source(
                "main.go",
                "/checkout/main.go",
                "package main\nfunc main() {}\n",
            )
            .unwrap(),
            input::SourceFileInput::from_source(
                "other.go",
                "/checkout/other.go",
                "package main\nfunc helper() {}\n",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let program = input::ProgramInput::standalone(
        input::WorkspaceKey::ad_hoc("compiler-tests").unwrap(),
        manifest,
    )
    .unwrap();

    let compiled = compile_program(program).expect("multi-file package compiles");
    assert!(compiled.modules.is_empty());
}

#[test]
fn program_boundary_requires_executable_main_signature() {
    for source in [
        "package library\nfunc main() {}\n",
        "package main\nfunc helper() {}\n",
        "package main\nfunc main(value int) {}\n",
        "package main\nfunc main() int { return 0 }\n",
    ] {
        let program = raw_program("main.go", "main.go", source);
        let error = compile_program(program)
            .err()
            .expect("invalid executable boundary rejected");
        assert_eq!(
            error.diagnostics().first().unwrap().code,
            "GORS2001",
            "{error}"
        );
        assert_eq!(
            error.diagnostics().first().unwrap().file,
            "main.go",
            "{error}"
        );
    }
}

#[test]
fn source_map_plans_are_independent_products() {
    let first = raw_program("first.go", "first.go", "package main\nfunc main() {}\n");
    let second = raw_program(
        "second.go",
        "second.go",
        "package main\nfunc main() { println(1) }\n",
    );
    let (first, first_plan) = compile_program_with_source_map(first).unwrap();
    let (second, second_plan) = compile_program_with_source_map(second).unwrap();
    let first_output = crate::printer::generate_single(first).unwrap();
    let second_output = crate::printer::generate_single(second).unwrap();
    let first_rust = first_output.files.get("main.rs").unwrap();
    let second_rust = second_output.files.get("main.rs").unwrap();

    let first_map = first_plan.build(first_rust);
    let second_map = second_plan.build(second_rust);

    assert_eq!(first_map.get_source(0), Some("first.go"));
    assert_eq!(second_map.get_source(0), Some("second.go"));
}
