use super::rust_ir::runtime_requirement as runtime_requirement_fingerprint;
use super::*;
use crate::compiler::ids::{DefinitionKey, DefinitionKind, IdentityInterner, QualifiedDefId};
use crate::compiler::input::{PackageKey, WorkspaceKey};
use crate::compiler::{self, hir, mir, rust_ir};
use gors_runtime_abi::{PrimitiveOp, RuntimeOp, RuntimeRequirement};

fn lower_stages(source: &str) -> (hir::File, mir::File, rust_ir::File) {
    lower_stages_at("fingerprint.go", source)
}

fn lower_stages_at(filename: &str, source: &str) -> (hir::File, mir::File, rust_ir::File) {
    let hir = compiler::lower_to_hir(filename, source).expect("typed HIR");
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
fn rust_ir_fingerprints_encode_exact_stable_operation_identities() {
    let (_, _, original) = lower_stages(
        r#"package main
func primitive(left int, right int) int { return left + right }
func runtime(left int, right int) int { return left / right }
func collision(left int, right int) int { return left &^ right }
func stable(left int, right int) int { return left | right }
"#,
    );
    let stable = rust_ir_function(rust_ir_named(&original, "stable"));

    let mut changed_primitive = original.clone();
    *value_op_mut(
        &mut changed_primitive,
        "primitive",
        rust_ir::ValueOp::Primitive(PrimitiveOp::IntWrappingAdd),
    ) = rust_ir::ValueOp::Primitive(PrimitiveOp::IntWrappingSub);
    assert_ne!(
        rust_ir_function(rust_ir_named(&original, "primitive")),
        rust_ir_function(rust_ir_named(&changed_primitive, "primitive"))
    );
    assert_ne!(rust_ir_file(&original), rust_ir_file(&changed_primitive));
    assert_eq!(
        stable,
        rust_ir_function(rust_ir_named(&changed_primitive, "stable"))
    );

    let mut changed_runtime = original.clone();
    *value_op_mut(
        &mut changed_runtime,
        "runtime",
        rust_ir::ValueOp::Runtime(RuntimeOp::IntDiv),
    ) = rust_ir::ValueOp::Runtime(RuntimeOp::IntRem);
    assert_ne!(
        rust_ir_function(rust_ir_named(&original, "runtime")),
        rust_ir_function(rust_ir_named(&changed_runtime, "runtime"))
    );
    assert_ne!(rust_ir_file(&original), rust_ir_file(&changed_runtime));
    assert_eq!(
        stable,
        rust_ir_function(rust_ir_named(&changed_runtime, "stable"))
    );

    assert_eq!(
        PrimitiveOp::IntAndNot.id().get(),
        RuntimeOp::IntDiv.id().get(),
        "the adversarial pair must collide numerically across operation domains"
    );
    let mut changed_domain = original.clone();
    *value_op_mut(
        &mut changed_domain,
        "collision",
        rust_ir::ValueOp::Primitive(PrimitiveOp::IntAndNot),
    ) = rust_ir::ValueOp::Runtime(RuntimeOp::IntDiv);
    assert_ne!(
        rust_ir_function(rust_ir_named(&original, "collision")),
        rust_ir_function(rust_ir_named(&changed_domain, "collision"))
    );
    assert_eq!(
        stable,
        rust_ir_function(rust_ir_named(&changed_domain, "stable"))
    );
}

#[test]
fn rust_ir_fingerprints_encode_hidden_and_terminal_runtime_operations() {
    let (_, _, original) = lower_stages(
        "package main\nfunc literal() string { return \"value\" }\nfunc main() { println(1) }\n",
    );

    let mut changed_constant = original.clone();
    *runtime_static_op_mut(
        &mut changed_constant,
        "literal",
        RuntimeOp::GoStringFromStatic,
    ) = RuntimeOp::GoStringFromBytes;
    assert_ne!(
        rust_ir_function(rust_ir_named(&original, "literal")),
        rust_ir_function(rust_ir_named(&changed_constant, "literal"))
    );
    assert_ne!(rust_ir_file(&original), rust_ir_file(&changed_constant));

    let mut changed_call = original.clone();
    *runtime_call_op_mut(&mut changed_call, "main", RuntimeOp::PrintNewline) =
        RuntimeOp::PrintSpace;
    assert_ne!(
        rust_ir_function(rust_ir_named(&original, "main")),
        rust_ir_function(rust_ir_named(&changed_call, "main"))
    );
    assert_ne!(rust_ir_file(&original), rust_ir_file(&changed_call));
}

#[test]
fn runtime_requirement_fingerprints_are_canonical_and_exact() {
    let left =
        RuntimeRequirement::new([RuntimeOp::PrintI64, RuntimeOp::IntDiv, RuntimeOp::PrintI64]);
    let reordered = RuntimeRequirement::new([RuntimeOp::IntDiv, RuntimeOp::PrintI64]);
    let changed = RuntimeRequirement::new([RuntimeOp::IntRem, RuntimeOp::PrintI64]);
    let missing = RuntimeRequirement::new([RuntimeOp::IntDiv]);

    assert_eq!(left, reordered);
    assert_eq!(
        runtime_requirement_fingerprint(&left),
        runtime_requirement_fingerprint(&reordered)
    );
    assert_ne!(
        runtime_requirement_fingerprint(&left),
        runtime_requirement_fingerprint(&changed)
    );
    assert_ne!(
        runtime_requirement_fingerprint(&left),
        runtime_requirement_fingerprint(&missing)
    );
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
fn source_refs_keep_stage_fingerprints_stable_across_whitespace_relocation() {
    let first =
        lower_stages("package main\nfunc stable(x int) int {\nreturn x + 1\n}\nfunc main() {}\n");
    let commented = lower_stages(
        "package main\nfunc stable(x int) int {\n// move the return anchor only\nreturn x + 1\n}\nfunc main() {}\n",
    );
    let changed =
        lower_stages("package main\nfunc stable(x int) int {\nreturn x + 2\n}\nfunc main() {}\n");
    let first_hir = hir_named(&first.0, "stable");
    let commented_hir = hir_named(&commented.0, "stable");
    let changed_hir = hir_named(&changed.0, "stable");
    let first_mir = mir_named(&first.1, "stable");
    let commented_mir = mir_named(&commented.1, "stable");
    let first_rust_ir = rust_ir_named(&first.2, "stable");
    let commented_rust_ir = rust_ir_named(&commented.2, "stable");

    assert_eq!(first_hir.source, commented_hir.source);
    assert_eq!(first_hir, commented_hir);
    assert_eq!(first_mir, commented_mir);
    assert_eq!(first_rust_ir, commented_rust_ir);
    assert_eq!(hir_function(first_hir), hir_function(commented_hir));
    assert_eq!(mir_function(first_mir), mir_function(commented_mir));
    assert_eq!(
        rust_ir_function(first_rust_ir),
        rust_ir_function(commented_rust_ir)
    );
    assert_ne!(hir_function(first_hir), hir_function(changed_hir));
}

#[test]
fn stage_fingerprints_exclude_presentation_paths() {
    let source = "package main\nfunc stable(x int) int { return x + 1 }\n";
    let first = lower_stages_at("/one/checkout/main.go", source);
    let moved = lower_stages_at(r"C:\different\checkout\main.go", source);

    assert_eq!(hir_file(&first.0), hir_file(&moved.0));
    assert_eq!(mir_file(&first.1), mir_file(&moved.1));
    assert_eq!(rust_ir_file(&first.2), rust_ir_file(&moved.2));
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

#[test]
fn qualified_definition_fingerprints_include_explicit_package_ownership() {
    let mut interner = IdentityInterner::default();
    let workspace = interner
        .workspace(&WorkspaceKey::ad_hoc("qualified-fingerprint-test").unwrap())
        .unwrap();
    let first_package = interner
        .package(
            workspace,
            &PackageKey::import_path("example/first").unwrap(),
        )
        .unwrap();
    let second_package = interner
        .package(
            workspace,
            &PackageKey::import_path("example/second").unwrap(),
        )
        .unwrap();
    let definition =
        DefinitionKey::package_named(first_package, DefinitionKind::Function, "Run").id();

    let first = QualifiedDefId::new(first_package, definition);
    let same = QualifiedDefId::new(first_package, definition);
    let different_owner = QualifiedDefId::new(second_package, definition);

    assert_eq!(qualified_definition(first), qualified_definition(same));
    assert_ne!(
        qualified_definition(first),
        qualified_definition(different_owner)
    );
}

fn value_op_mut<'a>(
    file: &'a mut rust_ir::File,
    function_name: &str,
    expected: rust_ir::ValueOp,
) -> &'a mut rust_ir::ValueOp {
    let function = file
        .functions
        .iter_mut()
        .find(|function| function.name == function_name)
        .expect("named Rust IR function");
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            match &mut statement.value.kind {
                rust_ir::RvalueKind::Unary { op, .. } | rust_ir::RvalueKind::Binary { op, .. }
                    if *op == expected =>
                {
                    return op;
                }
                rust_ir::RvalueKind::Use(_)
                | rust_ir::RvalueKind::Unary { .. }
                | rust_ir::RvalueKind::RecoverCompareNil { .. }
                | rust_ir::RvalueKind::Binary { .. } => {}
            }
        }
    }
    panic!("expected Rust IR value operation {expected:?}");
}

fn runtime_static_op_mut<'a>(
    file: &'a mut rust_ir::File,
    function_name: &str,
    expected: RuntimeOp,
) -> &'a mut RuntimeOp {
    let function = file
        .functions
        .iter_mut()
        .find(|function| function.name == function_name)
        .expect("named Rust IR function");
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            if let Some(operation) = rvalue_runtime_static_op_mut(&mut statement.value, expected) {
                return operation;
            }
        }
        if let Some(operation) = terminator_runtime_static_op_mut(&mut block.terminator, expected) {
            return operation;
        }
    }
    panic!("expected Rust IR runtime static-byte operation {expected:?}");
}

fn rvalue_runtime_static_op_mut(
    rvalue: &mut rust_ir::Rvalue,
    expected: RuntimeOp,
) -> Option<&mut RuntimeOp> {
    match &mut rvalue.kind {
        rust_ir::RvalueKind::Use(operand) | rust_ir::RvalueKind::Unary { operand, .. } => {
            operand_runtime_static_op_mut(operand, expected)
        }
        rust_ir::RvalueKind::Binary { left, right, .. } => {
            if let Some(operation) = operand_runtime_static_op_mut(left, expected) {
                Some(operation)
            } else {
                operand_runtime_static_op_mut(right, expected)
            }
        }
        rust_ir::RvalueKind::RecoverCompareNil { .. } => None,
    }
}

fn terminator_runtime_static_op_mut(
    terminator: &mut rust_ir::Terminator,
    expected: RuntimeOp,
) -> Option<&mut RuntimeOp> {
    match &mut terminator.kind {
        rust_ir::TerminatorKind::SwitchBool { condition, .. } => {
            operand_runtime_static_op_mut(condition, expected)
        }
        rust_ir::TerminatorKind::Call { args, .. } | rust_ir::TerminatorKind::Return(args) => args
            .iter_mut()
            .find_map(|operand| operand_runtime_static_op_mut(operand, expected)),
        rust_ir::TerminatorKind::Goto(_) | rust_ir::TerminatorKind::Unreachable => None,
    }
}

fn operand_runtime_static_op_mut(
    operand: &mut rust_ir::Operand,
    expected: RuntimeOp,
) -> Option<&mut RuntimeOp> {
    match operand {
        rust_ir::Operand::Constant(rust_ir::Constant::RuntimeStaticBytes { op, .. })
            if *op == expected =>
        {
            Some(op)
        }
        rust_ir::Operand::Read { .. } | rust_ir::Operand::Constant(_) | rust_ir::Operand::Unit => {
            None
        }
    }
}

fn runtime_call_op_mut<'a>(
    file: &'a mut rust_ir::File,
    function_name: &str,
    expected: RuntimeOp,
) -> &'a mut RuntimeOp {
    file.functions
        .iter_mut()
        .find(|function| function.name == function_name)
        .expect("named Rust IR function")
        .blocks
        .iter_mut()
        .find_map(|block| match &mut block.terminator.kind {
            rust_ir::TerminatorKind::Call {
                target: rust_ir::CallTarget::Runtime(operation),
                ..
            } if *operation == expected => Some(operation),
            rust_ir::TerminatorKind::Goto(_)
            | rust_ir::TerminatorKind::SwitchBool { .. }
            | rust_ir::TerminatorKind::Call { .. }
            | rust_ir::TerminatorKind::Return(_)
            | rust_ir::TerminatorKind::Unreachable => None,
        })
        .unwrap_or_else(|| panic!("expected Rust IR runtime call {expected:?}"))
}
