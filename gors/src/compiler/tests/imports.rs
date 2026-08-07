use super::raw_program;
use crate::compiler::{compile_program, input};

#[test]
fn unused_file_imports_are_rejected() {
    for (source, expected) in [
        (
            "package main\nimport \"unsafe\"\nfunc main() {}\n",
            "\"unsafe\" imported and not used",
        ),
        (
            "package main\nimport machine \"unsafe\"\nfunc main() {}\n",
            "\"unsafe\" imported as machine and not used",
        ),
    ] {
        let error = compile_program(raw_program("main.go", "main.go", source))
            .err()
            .expect("an unused ordinary import must be rejected");
        assert!(
            error.diagnostics().iter().any(|diagnostic| {
                diagnostic.message.contains(expected)
                    && diagnostic.code == "GORS2002"
                    && diagnostic.line == 2
            }),
            "missing {expected:?} at line 2 in {error}"
        );
    }
}

#[test]
fn used_and_blank_imports_stay_accepted() {
    for source in [
        "package main\nimport \"unsafe\"\nfunc main() { var x int; println(int(unsafe.Sizeof(x))) }\n",
        "package main\nimport _ \"unsafe\"\nfunc main() {}\n",
    ] {
        compile_program(raw_program("main.go", "main.go", source))
            .expect("a referenced or blank import must stay accepted");
    }
}

#[test]
fn import_usage_is_per_file_even_when_another_file_uses_the_package() {
    let unused_other = two_file_program(
        "package main\nimport \"unsafe\"\nfunc main() { var x int; println(int(unsafe.Sizeof(x))) }\n",
        "package main\nimport \"unsafe\"\nfunc helper() {}\n",
    );
    let error = compile_program(unused_other)
        .err()
        .expect("an import unused by its own file must be rejected");
    assert!(
        error.diagnostics().iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("\"unsafe\" imported and not used")
                && diagnostic.file == "/checkout/other.go"
        }),
        "missing per-file unused-import diagnostic in {error}"
    );

    let used_everywhere = two_file_program(
        "package main\nimport \"unsafe\"\nfunc main() { var x int; println(int(unsafe.Sizeof(x))) }\n",
        "package main\nimport \"unsafe\"\nfunc helper() int { var x int; return int(unsafe.Sizeof(x)) }\n",
    );
    compile_program(used_everywhere).expect("per-file used imports must stay accepted");
}

fn two_file_program(main_source: &str, other_source: &str) -> input::ProgramInput {
    let manifest = input::PackageInputManifest::new(
        input::PackageKey::command_line(),
        [
            input::SourceFileInput::from_source("main.go", "/checkout/main.go", main_source)
                .unwrap(),
            input::SourceFileInput::from_source("other.go", "/checkout/other.go", other_source)
                .unwrap(),
        ],
    )
    .unwrap();
    input::ProgramInput::standalone(
        input::WorkspaceKey::ad_hoc("compiler-tests").unwrap(),
        manifest,
    )
    .unwrap()
}
