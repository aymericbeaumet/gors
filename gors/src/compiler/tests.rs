use super::*;

struct GeneratedRun {
    rust: String,
    stderr: Vec<u8>,
}

fn raw_program(logical_path: &str, diagnostic_path: &str, source: &str) -> input::ProgramInput {
    let package = input::PackageKey::command_line();
    let file = input::SourceFileInput::from_source(logical_path, diagnostic_path, source).unwrap();
    let manifest = input::PackageInputManifest::new(package.clone(), [file]).unwrap();
    input::ProgramInput::new(
        input::WorkspaceKey::ad_hoc("compiler-tests").unwrap(),
        package,
        [manifest],
    )
    .unwrap()
}

fn compile_and_run(source: &str) -> GeneratedRun {
    let ast = crate::parser::parse_file("generated.go", source).unwrap();
    let rust_file = compile_file(&ast).expect("compile Go through verified MIR");
    let rust = prettyplease::unparse(&rust_file);
    let complete_source = crate::printer::generate_single(CompiledProgram {
        entry: rust_file,
        modules: BTreeMap::new(),
    })
    .expect("package generated Rust with the runtime ABI");
    let directory = tempfile::tempdir().expect("create generated-program directory");
    let source_path = directory.path().join("generated.rs");
    let binary_path = directory.path().join("generated-program");
    std::fs::write(&source_path, complete_source).expect("write generated Rust");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let compilation = std::process::Command::new(rustc)
        .arg("--edition=2024")
        .arg(&source_path)
        .arg("-o")
        .arg(&binary_path)
        .output()
        .expect("run rustc for generated program");
    assert!(
        compilation.status.success(),
        "generated Rust did not compile:\n{}\n{rust}",
        String::from_utf8_lossy(&compilation.stderr)
    );
    let output = std::process::Command::new(binary_path)
        .output()
        .expect("run generated program");
    assert!(
        output.status.success(),
        "generated program failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    GeneratedRun {
        rust,
        stderr: output.stderr,
    }
}

#[test]
fn stage_products_are_real_and_mandatory_lowering_is_deterministic() {
    let source = r#"
        package main
        func answer() int {
            if true {
                return 40 + 2
            }
            return 0
        }
        func main() { println(answer()) }
    "#;
    let ast = crate::parser::parse_file("stages.go", source).unwrap();
    let hir = lower_to_hir(&ast).expect("typed HIR");
    assert_eq!(hir.functions.len(), 2);

    let mir = lower_to_mir(&hir).expect("verified explicit-order MIR");
    assert!(
        mir.as_file()
            .functions
            .iter()
            .any(|function| function.blocks.len() > 1)
    );

    let rust_ir = lower_to_rust_ir(mir.clone()).expect("verified explicit Rust representation");
    let lowered_once = format!("{rust_ir:#?}");
    let rust_ir_again = lower_to_rust_ir(mir).expect("deterministic mandatory lowering");
    assert_eq!(format!("{rust_ir_again:#?}"), lowered_once);

    let rust = emit_rust_ir(&rust_ir).expect("terminal syn emission");
    assert!(rust.items.len() >= 2);
}

#[test]
fn generated_rust_executes_go_int_edge_semantics() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                maximum := 9223372036854775807
                minimum := -9223372036854775807 - 1
                one := 1
                minusOne := -1
                shift := 64
                println(maximum + one)
                println(minimum - one)
                println(maximum * 2)
                println(-minimum)
                println(minimum / minusOne)
                println(minimum % minusOne)
                println(one << shift)
                println(-2 >> shift)
            }
        "#,
    );

    assert_eq!(
        run.stderr,
        b"-9223372036854775808\n9223372036854775807\n-2\n-9223372036854775808\n-9223372036854775808\n0\n0\n-1\n"
    );
    for helper in [
        "int_add", "int_sub", "int_mul", "int_neg", "int_div", "int_rem", "int_shl", "int_shr",
    ] {
        assert!(run.rust.contains(helper), "missing runtime helper {helper}");
    }
}

#[test]
fn generated_rust_preserves_arbitrary_string_bytes() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                value := "\xff\x00A"
                println(value)
                println("\xff" < "\x00")
            }
        "#,
    );

    assert_eq!(
        run.stderr,
        [0xff, 0, b'A', b'\n', b'f', b'a', b'l', b's', b'e', b'\n']
    );
    assert!(run.rust.contains("go_string_from_static(b"), "{}", run.rust);
}

#[test]
fn generated_string_concatenation_preserves_aliases_and_empty_values() {
    let run = compile_and_run(
        r#"
            package main
            func twice(value string) string { return value + value }
            func grow(value string) string {
                value = value + "b"
                value = value + "c"
                return value
            }
            func main() {
                value := "\xff"
                copy := value
                println("" + "")
                println(twice(value))
                println(copy)
                println(grow("a"))
            }
        "#,
    );

    assert_eq!(
        run.stderr,
        [
            b'\n', 0xff, 0xff, b'\n', 0xff, b'\n', b'a', b'b', b'c', b'\n'
        ]
    );
}

#[test]
fn generated_rust_executes_verified_last_use_moves() {
    let run = compile_and_run(
        r#"
            package main
            func forward(value string) string { return value }
            func choose(flag bool, value string) string {
                saved := value
                if flag { println(value) }
                return saved
            }
            func main() {
                println(forward("move"))
                println(choose(true, "branch"))
                println(choose(false, "other"))
            }
        "#,
    );

    assert_eq!(run.stderr, b"move\nbranch\nbranch\nother\n");
    assert!(
        run.rust
            .contains(".take().expect(\"compiler move of uninitialized Go local\")"),
        "{}",
        run.rust
    );
    assert!(run.rust.contains(".clone()"), "{}", run.rust);
}

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
    let ast = crate::parser::parse_file("main.go", source).unwrap();
    let rust = crate::printer::generate(compile_file(&ast).unwrap()).unwrap();
    assert!(rust.contains("fn main"), "{rust}");
    assert!(rust.contains("42"), "{rust}");
}

#[test]
fn imports_fail_before_partial_codegen() {
    let source = "package main\nimport \"fmt\"\nfunc main() { fmt.Println(1) }\n";
    let ast = crate::parser::parse_file("main.go", source).unwrap();
    let Err(errors) = compile_file(&ast) else {
        panic!("imports must be a semantic diagnostic");
    };
    assert!(
        errors.iter().any(|error| error.message.contains("imports")),
        "{errors:?}"
    );
}

#[test]
fn program_boundary_rejects_multiple_independent_files() {
    let package = input::PackageKey::command_line();
    let manifest = input::PackageInputManifest::new(
        package.clone(),
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
    let program = input::ProgramInput::new(
        input::WorkspaceKey::ad_hoc("compiler-tests").unwrap(),
        package,
        [manifest],
    )
    .unwrap();

    let error = compile_program(program)
        .err()
        .expect("multi-file package rejected");

    assert_eq!(error.diagnostics().first().unwrap().code, "GORS2001");
    assert!(error.to_string().contains("exactly one"), "{error}");
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
    let first_rust = crate::printer::generate_single(first).unwrap();
    let second_rust = crate::printer::generate_single(second).unwrap();

    let first_map = first_plan.build(&first_rust);
    let second_map = second_plan.build(&second_rust);

    assert_eq!(first_map.get_source(0), Some("first.go"));
    assert_eq!(second_map.get_source(0), Some("second.go"));
}
