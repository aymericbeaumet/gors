use super::*;

fn lower(source: &str) -> File {
    let ast = crate::parser::parse_file("rust-ir.go", source).unwrap();
    let hir = crate::compiler::lower_to_hir(&ast).unwrap();
    let mir = crate::compiler::lower_to_mir(&hir).unwrap();
    crate::compiler::lower_to_rust_ir(mir)
        .unwrap()
        .as_file()
        .clone()
}

#[test]
fn representation_effects_cover_runtime_calls_clones_and_string_allocation() {
    let file = lower(
        r#"
            package main
            func join(left string, right string) string { return left + right }
            func add(left int, right int) int { return left + right }
            func main() { value := "x"; println(join(value, "y"), add(1, 2)) }
        "#,
    );

    let concat = binary_rvalue(&file, BinaryOp::StringConcat);
    assert!(concat.effects.may_read);
    assert!(concat.effects.may_call);
    assert!(concat.effects.may_allocate);
    assert!(!concat.effects.may_panic);
    assert_eq!(concat.panic, PanicEdge::None);

    let add = binary_rvalue(&file, BinaryOp::IntAdd);
    assert!(add.effects.may_read);
    assert!(add.effects.may_call);
    assert!(!add.effects.may_allocate);
    assert!(!add.effects.may_panic);

    let string_literal = all_rvalues(&file)
        .find(|rvalue| {
            matches!(
                rvalue.kind,
                RvalueKind::Use(Operand::Constant(Constant::GoString(_)))
            )
        })
        .unwrap();
    assert!(string_literal.effects.may_call);
    assert!(string_literal.effects.may_allocate);

    let clone_read = all_rvalues(&file)
        .find(|rvalue| {
            matches!(
                rvalue.kind,
                RvalueKind::Use(Operand::Read {
                    op: ReadOp::ProvenInitializedClone,
                    ..
                })
            )
        })
        .unwrap();
    assert!(clone_read.effects.may_read);
    assert!(clone_read.effects.may_call);
    assert!(clone_read.effects.may_allocate);
    assert!(!clone_read.effects.may_panic);
}

#[test]
fn verifier_rejects_noncanonical_print_plans() {
    let file = lower("package main\nfunc main() { println(1, 2) }\n");

    let mut reordered = file.clone();
    let steps = print_steps_mut(&mut reordered);
    steps.swap(0, 1);
    assert!(
        reordered
            .verify()
            .unwrap_err()
            .message
            .contains("exact canonical")
    );

    let mut no_final_newline = file;
    let steps = print_steps_mut(&mut no_final_newline);
    assert_eq!(steps.pop(), Some(PrintStep::PrintNewline));
    assert!(
        no_final_newline
            .verify()
            .unwrap_err()
            .message
            .contains("exact canonical")
    );

    let mut wrong_typed_abi = lower("package main\nfunc main() { print(1) }\n");
    print_steps_mut(&mut wrong_typed_abi)[0] = PrintStep::PrintBool { argument: 0 };
    assert!(
        wrong_typed_abi
            .verify()
            .unwrap_err()
            .message
            .contains("exact canonical")
    );
}

#[test]
fn verifier_rejects_a_store_removed_from_one_control_path() {
    let mut file = lower(
        r#"
            package main
            func choose(flag bool) int {
                value := 1
                if flag { value = 2 }
                return value
            }
        "#,
    );
    let function = file
        .functions
        .iter_mut()
        .find(|function| function.name == "choose")
        .unwrap();
    let value = function
        .locals
        .iter()
        .find(|local| local.name.as_deref() == Some("value"))
        .unwrap()
        .id;
    let mut removed = false;
    for block in &mut function.blocks {
        block.statements.retain(|statement| {
            if !removed && statement.destination.local == value {
                removed = true;
                false
            } else {
                true
            }
        });
    }
    assert!(removed);

    let error = file.verify().unwrap_err();
    assert!(error.message.contains("before initialization"), "{error:?}");
}

#[test]
fn verifier_rejects_mutated_representation_effects_and_panic_edges() {
    let source = r#"
        package main
        func calculate(left int, right int) int { return left / right }
        func join(left string, right string) string { return left + right }
    "#;
    let file = lower(source);

    let mut bad_allocation = file.clone();
    binary_rvalue_mut(&mut bad_allocation, BinaryOp::StringConcat)
        .effects
        .may_allocate = false;
    assert!(
        bad_allocation
            .verify()
            .unwrap_err()
            .message
            .contains("effect mismatch")
    );

    let mut bad_panic_edge = file.clone();
    binary_rvalue_mut(&mut bad_panic_edge, BinaryOp::IntDiv).panic = PanicEdge::None;
    assert!(
        bad_panic_edge
            .verify()
            .unwrap_err()
            .message
            .contains("panic edge mismatch")
    );

    let mut bad_provenance = file;
    binary_rvalue_mut(&mut bad_provenance, BinaryOp::IntDiv).provenance =
        Provenance::Source(crate::compiler::ids::SourceSpan::synthetic());
    assert!(
        bad_provenance
            .verify()
            .unwrap_err()
            .message
            .contains("source provenance")
    );
}

#[test]
fn verifier_rejects_mutated_function_artifact_plans() {
    let file = lower("package main\nfunc helper() {}\nfunc main() {}\n");
    let main_index = file
        .functions
        .iter()
        .position(|function| function.name == "main")
        .unwrap();
    let helper_index = file
        .functions
        .iter()
        .position(|function| function.name == "helper")
        .unwrap();

    let mut wrong_symbol = file.clone();
    wrong_symbol.functions[main_index].artifact.symbol = RustSymbol {
        spelling: "not_main".to_owned(),
    };
    assert!(
        wrong_symbol
            .verify()
            .unwrap_err()
            .message
            .contains("canonical main symbol")
    );

    let mut wrong_linkage = file.clone();
    wrong_linkage.functions[main_index].artifact.linkage = RustLinkage::Public;
    assert!(
        wrong_linkage
            .verify()
            .unwrap_err()
            .message
            .contains("internal linkage")
    );

    let mut missing_entrypoint_role = file.clone();
    missing_entrypoint_role.functions[main_index]
        .artifact
        .entrypoint = EntrypointPlan::None;
    assert!(
        missing_entrypoint_role
            .verify()
            .unwrap_err()
            .message
            .contains("canonical symbol")
    );

    let mut entrypoint_with_parameters = file.clone();
    entrypoint_with_parameters.functions[main_index]
        .signature
        .params
        .push(RustType::I64);
    assert!(
        entrypoint_with_parameters
            .verify()
            .unwrap_err()
            .message
            .contains("no parameters or results")
    );

    let mut duplicate_entrypoint = file;
    let entrypoint_plan = duplicate_entrypoint.functions[main_index].artifact.clone();
    duplicate_entrypoint.functions[helper_index].artifact = entrypoint_plan;
    assert!(
        duplicate_entrypoint
            .verify()
            .unwrap_err()
            .message
            .contains("multiple Rust IR executable entrypoints")
    );
}

#[test]
fn terminal_emission_uses_the_verified_artifact_plan_not_go_name_text() {
    let mut file = lower("package main\nfunc helper() {}\nfunc main() {}\n");
    let main = file
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .unwrap();
    main.name = "diagnostic_entry_name".to_owned();
    let helper = file
        .functions
        .iter_mut()
        .find(|function| function.name == "helper")
        .unwrap();
    helper.name = "diagnostic_helper_name".to_owned();

    file.verify().unwrap();
    let syntax = crate::compiler::emit::emit_file(&file).unwrap();
    let rust = prettyplease::unparse(&syntax);

    assert!(rust.contains("fn main()"), "{rust}");
    assert!(rust.contains("pub fn __gors_fn_0()"), "{rust}");
    assert!(!rust.contains("diagnostic_entry_name"), "{rust}");
    assert!(!rust.contains("diagnostic_helper_name"), "{rust}");
}

#[test]
fn lowering_selects_entrypoints_from_package_role_before_emission() {
    let executable = lower("package main\nfunc main() {}\n");
    let executable_main = &executable.functions[0];
    assert_eq!(
        executable_main.artifact.entrypoint,
        EntrypointPlan::Executable
    );
    assert_eq!(executable_main.artifact.linkage, RustLinkage::Internal);
    assert_eq!(executable_main.artifact.symbol.as_str(), "main");

    let library = lower("package library\nfunc main() {}\n");
    let library_main = &library.functions[0];
    assert_eq!(library_main.artifact.entrypoint, EntrypointPlan::None);
    assert_eq!(library_main.artifact.linkage, RustLinkage::Public);
    assert_eq!(library_main.artifact.symbol.as_str(), "__gors_fn_0");

    let syntax = crate::compiler::emit::emit_file(&library).unwrap();
    let rust = prettyplease::unparse(&syntax);
    assert!(rust.contains("pub fn __gors_fn_0()"), "{rust}");
    assert!(!rust.contains("fn main()"), "{rust}");
}

fn all_rvalues(file: &File) -> impl Iterator<Item = &Rvalue> {
    file.functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.statements)
        .map(|statement| &statement.value)
}

fn binary_rvalue(file: &File, expected: BinaryOp) -> &Rvalue {
    all_rvalues(file)
        .find(|rvalue| matches!(rvalue.kind, RvalueKind::Binary { op, .. } if op == expected))
        .unwrap()
}

fn binary_rvalue_mut(file: &mut File, expected: BinaryOp) -> &mut Rvalue {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.statements)
        .map(|statement| &mut statement.value)
        .find(|rvalue| matches!(rvalue.kind, RvalueKind::Binary { op, .. } if op == expected))
        .unwrap()
}

fn print_steps_mut(file: &mut File) -> &mut Vec<PrintStep> {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .find_map(|block| match &mut block.terminator.kind {
            TerminatorKind::Call {
                target: CallTarget::RuntimePrint { steps },
                ..
            } => Some(steps),
            _ => None,
        })
        .unwrap()
}
