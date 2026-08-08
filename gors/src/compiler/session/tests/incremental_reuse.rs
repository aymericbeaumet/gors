//! Incremental reuse and language-version regression coverage.

use super::*;

#[test]
fn syntax_invalid_input_is_query_owned_and_repeated_revision_is_green() {
    let program = raw_program(
        "main.go",
        "/checkout/main.go",
        "package main\nfunc main( {\n",
    );
    let mut session = CompilerSession::default();

    let first = session
        .compile_program(program.clone())
        .err()
        .expect("syntax-invalid raw source must fail in the parse query");
    assert_eq!(first.diagnostics().first().unwrap().code, "GORS2002");
    assert_eq!(
        first.diagnostics().first().unwrap().file,
        "/checkout/main.go"
    );
    assert_eq!(session.database().active_files().len(), 1);

    session.database().reset_telemetry();
    let second = session
        .compile_program(program)
        .err()
        .expect("the unchanged invalid revision must remain invalid");
    assert_eq!(second, first);
    assert_eq!(session.database().telemetry().total_executions(), 0);
}

#[test]
fn binary_literals_obey_package_and_file_language_versions() {
    let source = "package main\n\nfunc main() {\n\tx := 0b1011\n\tprintln(x)\n}\n";
    let old = raw_program_at_version(
        "main.go",
        "/checkout/project/main.go",
        source,
        GoLanguageVersion::new(1, 12),
    );
    let mut session = CompilerSession::default();
    let error = session
        .compile_program(old)
        .err()
        .expect("a binary literal must be rejected under Go 1.12");
    let diagnostic = error.diagnostics().first().unwrap();
    assert_eq!(diagnostic.code, "GORS2002");
    assert_eq!(diagnostic.file, "/checkout/project/main.go");
    assert_eq!((diagnostic.line, diagnostic.column), (4, 7));
    assert_eq!(
        diagnostic.message,
        "binary literal requires go1.13 or later (-lang was set to go1.12; check go.mod)"
    );

    session
        .compile_program(raw_program_at_version(
            "main.go",
            "/checkout/project/main.go",
            source,
            GoLanguageVersion::new(1, 13),
        ))
        .expect("Go 1.13 accepts binary literals");

    let file_override = "//go:build go1.13\n\npackage main\n\nfunc main() { println(0b1011) }\n";
    session
        .compile_program(raw_program_at_version(
            "main.go",
            "/checkout/project/main.go",
            file_override,
            GoLanguageVersion::new(1, 12),
        ))
        .expect("a Go version build constraint selects the file language version");
}

#[test]
fn language_version_only_updates_reuse_projection_and_semantic_products() {
    let source = "package main\nfunc main() { println(0b1011) }\n";
    let mut session = CompilerSession::default();
    session
        .compile_program(raw_program_at_version(
            "main.go",
            "/checkout/project/main.go",
            source,
            GoLanguageVersion::new(1, 13),
        ))
        .unwrap();
    session.database().reset_telemetry();

    session
        .compile_program(raw_program_at_version(
            "main.go",
            "/checkout/project/main.go",
            source,
            GoLanguageVersion::new(1, 14),
        ))
        .unwrap();

    let telemetry = session.database().telemetry();
    assert_eq!(
        telemetry.executions(crate::compiler::db::QueryKind::FileProjection),
        0
    );
    assert_eq!(
        telemetry.executions(crate::compiler::db::QueryKind::LanguageVersionCheck),
        1
    );
    assert_eq!(
        telemetry.executions(crate::compiler::db::QueryKind::TypedHir),
        0
    );
    assert_eq!(
        telemetry.executions(crate::compiler::db::QueryKind::VerifiedGoMir),
        0
    );
    assert_eq!(
        telemetry.executions(crate::compiler::db::QueryKind::VerifiedRustIr),
        0
    );

    session.database().reset_telemetry();
    session
        .compile_program(raw_program_at_version(
            "main.go",
            "/checkout/project/main.go",
            source,
            GoLanguageVersion::new(1, 14),
        ))
        .unwrap();
    assert_eq!(session.database().telemetry().total_executions(), 0);
}

#[test]
fn runtime_int32_wrapping_lowering_is_incrementally_reused() {
    let program = raw_program(
        "main.go",
        "/checkout/main.go",
        "package main\nfunc next(value int32) int32 { return value + 1 }\nfunc negate(value rune) rune { return -value }\nfunc main() { println(next(2147483647), negate(-2147483647-1)) }\n",
    );
    let mut session = CompilerSession::default();

    session
        .compile_program(program.clone())
        .expect("runtime int32/rune wrapping must compile");

    session.database().reset_telemetry();
    session
        .compile_program(program)
        .expect("the unchanged int32/rune revision must remain valid");

    assert_eq!(session.database().telemetry().total_executions(), 0);
}

#[test]
fn float32_body_edits_invalidate_only_the_owning_numeric_root() {
    let program = |increment: &str| {
        raw_program(
            "main.go",
            "/checkout/main.go",
            &format!(
                "package main\nfunc changed(value float32) float32 {{ return value + {increment} }}\nfunc stable(value float64) float64 {{ return value + 1 }}\nfunc main() {{ println(changed(1), stable(1)) }}\n"
            ),
        )
    };
    let mut session = CompilerSession::default();
    session
        .compile_program(program("1"))
        .expect("initial float-width program must compile");
    let scheduled = session.scheduler_telemetry().scheduled_roots;
    session.database().reset_telemetry();

    let changed = program("2");
    session
        .compile_program(changed.clone())
        .expect("edited float32 root must compile");

    assert_eq!(session.scheduler_telemetry().scheduled_roots, scheduled + 1);
    let telemetry = session.database().telemetry();
    for kind in [
        crate::compiler::db::QueryKind::TypedHir,
        crate::compiler::db::QueryKind::VerifiedGoMir,
        crate::compiler::db::QueryKind::NormalizedGoMir,
        crate::compiler::db::QueryKind::VerifiedRustIr,
    ] {
        assert_eq!(telemetry.executions(kind), 1, "{kind:?}");
    }

    session.database().reset_telemetry();
    session
        .compile_program(changed)
        .expect("unchanged float32 revision must remain green");
    assert_eq!(session.database().telemetry().total_executions(), 0);
}

#[test]
fn unrelated_method_body_edits_leave_promoted_receiver_callers_green() {
    let program = |unrelated: &str| {
        raw_program(
            "main.go",
            "/checkout/main.go",
            &format!(
                "package main\ntype Inner struct {{ value int }}\nfunc (inner Inner) Read() int {{ return inner.value }}\ntype Outer struct {{ Inner }}\nfunc (outer Outer) Unrelated() int {{ return {unrelated} }}\nfunc selected(outer Outer) int {{ return outer.Read() }}\nfunc main() {{ println(selected(Outer{{Inner: Inner{{value: 3}}}})) }}\n"
            ),
        )
    };
    let mut session = CompilerSession::default();
    session
        .compile_program(program("1"))
        .expect("initial promoted receiver program must compile");
    let scheduled = session.scheduler_telemetry().scheduled_roots;
    session.database().reset_telemetry();

    let changed = program("2");
    session
        .compile_program(changed.clone())
        .expect("unrelated method body edit must compile");

    assert_eq!(session.scheduler_telemetry().scheduled_roots, scheduled + 1);
    let telemetry = session.database().telemetry();
    for kind in [
        crate::compiler::db::QueryKind::TypedHir,
        crate::compiler::db::QueryKind::VerifiedGoMir,
        crate::compiler::db::QueryKind::NormalizedGoMir,
        crate::compiler::db::QueryKind::VerifiedRustIr,
    ] {
        assert_eq!(telemetry.executions(kind), 1, "{kind:?}");
    }

    session.database().reset_telemetry();
    session
        .compile_program(changed)
        .expect("unchanged promoted receiver revision must remain green");
    assert_eq!(session.database().telemetry().total_executions(), 0);
}

#[test]
fn goto_scope_diagnostic_is_incrementally_reused() {
    let program = raw_program(
        "main.go",
        "/checkout/main.go",
        "package main\nfunc main() {\n\tgoto Nested\n\t{\n\tNested:\n\t}\n}\n",
    );
    let mut session = CompilerSession::default();

    let first = session
        .compile_program(program.clone())
        .err()
        .expect("goto into a nested block must fail semantic analysis");
    let diagnostic = first.diagnostics().first().unwrap();
    assert_eq!(diagnostic.code, "GORS2002");
    assert_eq!(diagnostic.message, "goto Nested jumps into block");

    session.database().reset_telemetry();
    let second = session
        .compile_program(program)
        .err()
        .expect("the unchanged invalid goto must remain invalid");

    assert_eq!(second, first);
    assert_eq!(session.database().telemetry().total_executions(), 0);
}

#[test]
fn multi_argument_generic_instantiation_is_incrementally_reused() {
    let program = raw_program(
        "main.go",
        "/checkout/main.go",
        "package main\nfunc second[A, B any](a A, b B) B { return b }\nfunc main() { println(second[int, string](1, \"two\")) }\n",
    );
    let mut session = CompilerSession::default();

    session
        .compile_program(program.clone())
        .expect("multi-argument generic instantiation must compile");

    session.database().reset_telemetry();
    session
        .compile_program(program)
        .expect("the unchanged generic revision must remain valid");

    assert_eq!(session.database().telemetry().total_executions(), 0);
}
