use std::collections::BTreeMap;
use std::sync::Arc;

use crate::compiler::ids::SourceSpan;
use crate::parser::SourceSnapshot;

use super::*;

#[test]
fn install_rollback_restores_updated_inputs_and_removes_orphans() {
    let mut session = CompilerSession::default();
    let original = Arc::new(SourceSnapshot::from_source(
        "/original/main.go",
        "package main\nfunc main() {}\n",
    ));
    let file = session
        .database
        .set_source(
            WORKSPACE_IDENTITY,
            "command-line-package:main",
            "main.go",
            Arc::clone(&original),
        )
        .unwrap();
    let previous = BTreeMap::from([(file, Arc::clone(&original))]);

    session
        .database
        .set_source(
            WORKSPACE_IDENTITY,
            "command-line-package:main",
            "main.go",
            Arc::new(SourceSnapshot::from_source(
                "/failed/main.go",
                "package main\nfunc main() { println(1) }\n",
            )),
        )
        .unwrap();
    let orphan = session
        .database
        .set_source(
            WORKSPACE_IDENTITY,
            "command-line-package:main",
            "orphan.go",
            Arc::new(SourceSnapshot::from_source(
                "/failed/orphan.go",
                "package main\nfunc orphan() {}\n",
            )),
        )
        .unwrap();

    session.rollback_install(&previous);

    assert_eq!(session.database.active_files(), vec![file]);
    assert_eq!(
        session.database.source_snapshot(file).unwrap().as_ref(),
        original.as_ref()
    );
    assert!(session.database.source_snapshot(orphan).is_err());
}

#[test]
fn function_relative_diagnostic_rebases_to_current_anchor() {
    let source = "package main\n\nfunc prefix() {}\n\nfunc target() {}\nfunc main() {}\n";
    let mut session = CompilerSession::default();
    session
        .compile_program(crate::parser::parse_program_from_source("main.go", source).unwrap())
        .unwrap();
    let file = session.database.active_files().first().copied().unwrap();
    let analysis = session.database.analyze_file(file).unwrap();
    let target = analysis
        .functions()
        .iter()
        .find(|function| function.name() == "target")
        .unwrap();
    let provenance = session
        .database
        .function_provenance(file, target.id())
        .unwrap();
    let mut diagnostic = super::super::Diagnostic {
        code: "GORS2003",
        message: "test diagnostic".to_string(),
        span: SourceSpan {
            file: "main.go".to_string(),
            start: 2,
            end: 4,
            line: 2,
            column: 3,
        },
    };

    rebase_function_diagnostic(&mut diagnostic, &provenance);

    assert_eq!(diagnostic.span.start, provenance.byte_offset() + 2);
    assert_eq!(diagnostic.span.end, provenance.byte_offset() + 4);
    assert_eq!(diagnostic.span.line, provenance.line() + 1);
    assert_eq!(diagnostic.span.column, 3);
}
