use super::*;
use crate::compiler::{self, hir, mir, rust_ir};

fn lower_stages(source: &str) -> (hir::File, mir::File, rust_ir::File) {
    let parsed = crate::parser::parse_file("fingerprint.go", source).expect("valid test source");
    let hir = compiler::lower_to_hir(&parsed).expect("typed HIR");
    let verified_mir = compiler::lower_to_mir(&hir).expect("verified Go MIR");
    let mir = verified_mir.as_file().clone();
    let rust_ir = compiler::lower_to_rust_ir(verified_mir)
        .expect("verified Rust IR")
        .as_file()
        .clone();
    (hir, mir, rust_ir)
}

fn hir_named<'a>(file: &'a hir::File, name: &str) -> &'a hir::Function {
    file.functions
        .iter()
        .find(|function| function.name == name)
        .expect("named HIR function")
}

fn mir_named<'a>(file: &'a mir::File, name: &str) -> &'a mir::Function {
    file.functions
        .iter()
        .find(|function| function.name == name)
        .expect("named MIR function")
}

fn rust_ir_named<'a>(file: &'a rust_ir::File, name: &str) -> &'a rust_ir::Function {
    file.functions
        .iter()
        .find(|function| function.name == name)
        .expect("named Rust IR function")
}

#[test]
fn structural_clones_have_identical_stage_fingerprints() {
    let (hir, mir, rust_ir) = lower_stages(
        "package main\nfunc value(x int) int { return x + 1 }\nfunc main() { println(value(4)) }\n",
    );

    assert_eq!(hir, hir.clone());
    assert_eq!(mir, mir.clone());
    assert_eq!(rust_ir, rust_ir.clone());
    assert_eq!(hir_file(&hir), hir_file(&hir.clone()));
    assert_eq!(mir_file(&mir), mir_file(&mir.clone()));
    assert_eq!(rust_ir_file(&rust_ir), rust_ir_file(&rust_ir.clone()));
    assert_eq!(hir_file(&hir).as_bytes().len(), 32);
    assert_eq!(hir_file(&hir).to_hex().len(), 64);
}

#[test]
fn semantic_field_mutations_change_each_stage_fingerprint() {
    let source =
        "package main\nfunc value(x int) int { return x + 1 }\nfunc main() { println(value(4)) }\n";
    let (hir, mir, rust_ir) = lower_stages(source);

    let mut changed_hir = hir.clone();
    let hir_function = changed_hir.functions.first_mut().expect("HIR function");
    let hir_statement = hir_function
        .body
        .stmts
        .first_mut()
        .expect("HIR return statement");
    let hir::StmtKind::Return(values) = &mut hir_statement.kind else {
        panic!("expected HIR return statement");
    };
    let expression = values.first_mut().expect("HIR return expression");
    expression.effects.may_panic = !expression.effects.may_panic;
    assert_ne!(hir, changed_hir);
    assert_ne!(hir_file(&hir), hir_file(&changed_hir));

    let mut changed_mir = mir.clone();
    let terminator = &mut changed_mir
        .functions
        .first_mut()
        .expect("MIR function")
        .blocks
        .first_mut()
        .expect("MIR block")
        .terminator;
    terminator.effects.may_allocate = !terminator.effects.may_allocate;
    assert_ne!(mir, changed_mir);
    assert_ne!(mir_file(&mir), mir_file(&changed_mir));

    let mut changed_rust_ir = rust_ir.clone();
    let terminator = &mut changed_rust_ir
        .functions
        .first_mut()
        .expect("Rust IR function")
        .blocks
        .first_mut()
        .expect("Rust IR block")
        .terminator;
    terminator.effects.may_allocate = !terminator.effects.may_allocate;
    assert_ne!(rust_ir, changed_rust_ir);
    assert_ne!(rust_ir_file(&rust_ir), rust_ir_file(&changed_rust_ir));
}

#[test]
fn rust_ir_fingerprints_distinguish_clone_and_move_read_plans() {
    let (_, _, rust_ir) =
        lower_stages("package main\nfunc twice(value string) { print(value); print(value) }\n");
    let mut changed = rust_ir.clone();
    let read = changed
        .functions
        .iter_mut()
        .find(|function| function.name == "twice")
        .into_iter()
        .flat_map(|function| &mut function.blocks)
        .find_map(|block| match &mut block.terminator.kind {
            rust_ir::TerminatorKind::Call { args, .. } => args.iter_mut().find_map(|argument| {
                let rust_ir::Operand::Read { op, .. } = argument else {
                    return None;
                };
                (*op == rust_ir::ReadOp::ProvenLastUseMove).then_some(op)
            }),
            _ => None,
        })
        .expect("last-use move read");
    *read = rust_ir::ReadOp::ProvenInitializedClone;

    assert_ne!(rust_ir, changed);
    assert_ne!(rust_ir_file(&rust_ir), rust_ir_file(&changed));
}

#[test]
fn function_fingerprints_ignore_unrelated_sibling_order() {
    let first = lower_stages(
        "package main\nfunc stable(x int) int { return x + 1 }\nfunc alpha() int { return 2 }\nfunc beta() int { return 3 }\nfunc main() { println(stable(4)) }\n",
    );
    let reordered = lower_stages(
        "package main\nfunc stable(x int) int { return x + 1 }\nfunc beta() int { return 3 }\nfunc alpha() int { return 2 }\nfunc main() { println(stable(4)) }\n",
    );

    assert_eq!(
        hir_function(hir_named(&first.0, "stable")),
        hir_function(hir_named(&reordered.0, "stable"))
    );
    assert_eq!(
        mir_function(mir_named(&first.1, "stable")),
        mir_function(mir_named(&reordered.1, "stable"))
    );
    assert_eq!(
        rust_ir_function(rust_ir_named(&first.2, "stable")),
        rust_ir_function(rust_ir_named(&reordered.2, "stable"))
    );
}

#[test]
fn stage_domains_separate_analogous_file_payloads() {
    let hir = hir::File {
        package: "main".into(),
        constants: Vec::new(),
        functions: Vec::new(),
    };
    let mir = mir::File {
        package: "main".into(),
        functions: Vec::new(),
    };
    let rust_ir = rust_ir::File {
        package: "main".into(),
        functions: Vec::new(),
    };

    let hir = hir_file(&hir);
    let mir = mir_file(&mir);
    let rust_ir = rust_ir_file(&rust_ir);
    assert_ne!(hir, mir);
    assert_ne!(hir, rust_ir);
    assert_ne!(mir, rust_ir);
}
