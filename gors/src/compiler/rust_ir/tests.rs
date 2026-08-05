use super::*;

fn lower(source: &str) -> File {
    lower_at("rust-ir.go", source)
}

fn lower_at(filename: &str, source: &str) -> File {
    let hir = crate::compiler::lower_to_hir(filename, source).unwrap();
    let mir = crate::compiler::lower_to_mir(&hir).unwrap();
    crate::compiler::lower_to_rust_ir(mir)
        .unwrap()
        .as_file()
        .clone()
}

#[test]
fn generated_symbols_do_not_embed_checkout_paths() {
    let source =
        "package main\nfunc helper() int { return 42 }\nfunc main() { println(helper()) }\n";
    let symbols = |filename| {
        lower_at(filename, source)
            .functions
            .into_iter()
            .map(|function| function.artifact.symbol.spelling)
            .collect::<Vec<_>>()
    };
    let portable = symbols("/one/checkout/main.go");
    assert_eq!(portable, symbols("/different/root/main.go"));
    assert_eq!(portable, symbols(r"C:\different\root\main.go"));
}

#[test]
fn representation_effects_cover_runtime_calls_clones_and_string_allocation() {
    let file = lower(
        r#"
            package main
            func join(left string, right string) string { return left + right }
            func identity(value string) string { saved := value; return saved }
            func add(left int, right int) int { return left + right }
            func equal(left string, right string) bool { return left == right }
            func notEqual(left string, right string) bool { return left != right }
            func less(left string, right string) bool { return left < right }
            func lessEqual(left string, right string) bool { return left <= right }
            func greater(left string, right string) bool { return left > right }
            func greaterEqual(left string, right string) bool { return left >= right }
            func main() {
                value := "x"
                saved := value
                println(join(value, "y"), add(1, 2))
                println(saved)
            }
        "#,
    );

    let concat = binary_rvalue(&file, ValueOp::Runtime(RuntimeOp::ConcatGoStrings));
    assert!(concat.effects.may_read);
    assert!(concat.effects.may_call);
    assert!(concat.effects.may_allocate);
    assert!(concat.effects.may_write);
    assert!(!concat.effects.may_panic);
    assert_eq!(concat.panic, PanicEdge::None);

    let add = binary_rvalue(&file, ValueOp::Primitive(PrimitiveOp::IntWrappingAdd));
    assert!(add.effects.may_read);
    assert!(!add.effects.may_call);
    assert!(!add.effects.may_allocate);
    assert!(!add.effects.may_panic);

    for operation in [
        PrimitiveOp::StringEqual,
        PrimitiveOp::StringNotEqual,
        PrimitiveOp::StringLess,
        PrimitiveOp::StringLessEqual,
        PrimitiveOp::StringGreater,
        PrimitiveOp::StringGreaterEqual,
    ] {
        let comparison = binary_rvalue(&file, ValueOp::Primitive(operation));
        assert!(comparison.effects.may_read);
        assert!(comparison.effects.may_call);
        assert!(!comparison.effects.may_allocate);
        assert!(!comparison.effects.may_panic);
    }

    let string_literal = all_rvalues(&file)
        .find(|rvalue| {
            matches!(
                rvalue.kind,
                RvalueKind::Use(Operand::Constant(Constant::RuntimeStaticBytes {
                    op: RuntimeOp::GoStringFromStatic,
                    ..
                }))
            )
        })
        .unwrap();
    assert!(string_literal.effects.may_call);
    assert!(!string_literal.effects.may_allocate);

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
    assert!(!clone_read.effects.may_allocate);
    assert!(!clone_read.effects.may_panic);

    let move_read = all_rvalues(&file)
        .find(|rvalue| {
            matches!(
                rvalue.kind,
                RvalueKind::Use(Operand::Read {
                    op: ReadOp::ProvenLastUseMove,
                    ..
                })
            )
        })
        .unwrap();
    assert!(move_read.effects.may_read);
    assert!(move_read.effects.may_write);
    assert!(!move_read.effects.may_call);
    assert!(!move_read.effects.may_allocate);
    assert!(!move_read.effects.may_panic);
    assert_eq!(move_read.panic, PanicEdge::None);
}

#[test]
fn verifier_checks_runtime_calls_against_the_typed_abi() {
    let mut file = lower("package main\nfunc main() { print(1) }\n");
    *runtime_call_target_mut(&mut file, RuntimeOp::PrintI64) = RuntimeOp::PrintBool;
    refresh_test_effects(&mut file);

    let error = file.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("runtime call argument 0 type mismatch"),
        "{error:?}"
    );
}

#[test]
fn runtime_call_destination_writes_are_explicit_and_verified() {
    let mut file = lower(
        r#"
            package main
            func helper(left int, right int) int { return left + right }
            func main() { value := helper(4, 2); print(value) }
        "#,
    );
    let terminator = first_function_call_terminator_mut(&mut file);
    if let TerminatorKind::Call { target, .. } = &mut terminator.kind {
        *target = CallTarget::Runtime(RuntimeOp::IntDiv);
    }
    refresh_test_effects(&mut file);

    assert!(
        runtime_call_terminator_mut(&mut file, RuntimeOp::IntDiv)
            .effects
            .may_write
    );
    file.verify().unwrap();

    runtime_call_terminator_mut(&mut file, RuntimeOp::IntDiv)
        .effects
        .may_write = false;
    let error = file.verify().unwrap_err();
    assert!(
        error.message.contains("terminator effect mismatch"),
        "{error:?}"
    );
}

#[test]
fn verifier_checks_value_operations_against_the_typed_abi() {
    let mut file = lower(
        "package main\nfunc add(left int, right int) int { return left + right }\nfunc main() {}\n",
    );
    *binary_value_op_mut(&mut file, ValueOp::Primitive(PrimitiveOp::IntWrappingAdd)) =
        ValueOp::Primitive(PrimitiveOp::BoolEqual);
    refresh_test_effects(&mut file);

    let error = file.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("binary operation argument 0 type mismatch"),
        "{error:?}"
    );
}

#[test]
fn verifier_rejects_static_bytes_with_an_incompatible_runtime_constructor() {
    let mut file = lower("package main\nfunc main() { print(\"value\") }\n");
    *static_bytes_runtime_op_mut(&mut file) = RuntimeOp::IntDiv;

    let error = file.verify().unwrap_err();
    assert!(
        error.message.contains("static bytes use runtime operation"),
        "{error:?}"
    );
}

#[test]
fn verifier_returns_the_canonical_runtime_requirement() {
    let file = lower(
        r#"
            package main
            func join(left string, right string) string { return left + right }
            func main() { println(join("a", "b")) }
        "#,
    );

    let requirement = file.verify().unwrap();
    assert_eq!(
        requirement,
        RuntimeRequirement::new([
            RuntimeOp::GoStringFromStatic,
            RuntimeOp::ConcatGoStrings,
            RuntimeOp::PrintGoString,
            RuntimeOp::PrintNewline,
        ])
    );
}

#[test]
fn zero_argument_print_requires_no_runtime_operation() {
    let file = lower("package main\nfunc main() { print() }\n");

    assert!(file.verify().unwrap().is_empty());
    assert!(runtime_operations_in_execution_order(&file, "main").is_empty());
}

#[test]
fn zero_argument_println_requires_only_newline() {
    let file = lower("package main\nfunc main() { println() }\n");

    assert_eq!(
        file.verify().unwrap(),
        RuntimeRequirement::new([RuntimeOp::PrintNewline])
    );
    assert_eq!(
        runtime_operations_in_execution_order(&file, "main"),
        [RuntimeOp::PrintNewline]
    );
}

#[test]
fn println_expands_to_ordered_single_operation_runtime_calls() {
    let file = lower(
        r#"
            package main
            func main() { println(1, true, "x") }
        "#,
    );

    assert_eq!(
        runtime_operations_in_execution_order(&file, "main"),
        [
            RuntimeOp::PrintI64,
            RuntimeOp::PrintSpace,
            RuntimeOp::PrintBool,
            RuntimeOp::PrintSpace,
            RuntimeOp::PrintGoString,
            RuntimeOp::PrintNewline,
        ]
    );
    file.verify().unwrap();
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
fn verifier_rejects_noncanonical_moves_and_clones() {
    let source = r#"
        package main
        func twice(value string) {
            print(value)
            print(value)
        }
    "#;

    let mut premature_move = lower(source);
    *read_op_mut(&mut premature_move, "twice", ReadOp::ProvenInitializedClone).unwrap() =
        ReadOp::ProvenLastUseMove;
    refresh_test_effects(&mut premature_move);
    let error = premature_move.verify().unwrap_err();
    assert!(error.message.contains("CFG liveness requires"), "{error:?}");
    assert!(
        error.message.contains("ProvenInitializedClone"),
        "{error:?}"
    );

    let mut unnecessary_clone = lower(source);
    *read_op_mut(&mut unnecessary_clone, "twice", ReadOp::ProvenLastUseMove).unwrap() =
        ReadOp::ProvenInitializedClone;
    refresh_test_effects(&mut unnecessary_clone);
    let error = unnecessary_clone.verify().unwrap_err();
    assert!(error.message.contains("CFG liveness requires"), "{error:?}");
    assert!(error.message.contains("ProvenLastUseMove"), "{error:?}");
}

#[test]
fn verifier_rejects_mutated_representation_effects_and_panic_edges() {
    let source = r#"
        package main
        func unrelated() {}
        func calculate(left int, right int) int { return left / right }
        func join(left string, right string) string { return left + right }
    "#;
    let file = lower(source);

    let mut bad_allocation = file.clone();
    binary_rvalue_mut(
        &mut bad_allocation,
        ValueOp::Runtime(RuntimeOp::ConcatGoStrings),
    )
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
    binary_rvalue_mut(&mut bad_panic_edge, ValueOp::Runtime(RuntimeOp::IntDiv)).panic =
        PanicEdge::None;
    assert!(
        bad_panic_edge
            .verify()
            .unwrap_err()
            .message
            .contains("panic edge mismatch")
    );

    let unrelated_source = file
        .functions
        .iter()
        .find(|function| function.name == "unrelated")
        .unwrap()
        .source;
    let mut bad_provenance = file.clone();
    binary_rvalue_mut(&mut bad_provenance, ValueOp::Runtime(RuntimeOp::IntDiv)).provenance =
        Provenance::Source(unrelated_source);
    assert!(
        bad_provenance
            .verify()
            .unwrap_err()
            .message
            .contains("source reference is owned by")
    );

    let mut bad_function_source = file;
    bad_function_source
        .functions
        .iter_mut()
        .find(|function| function.name == "calculate")
        .unwrap()
        .source = unrelated_source;
    assert!(
        bad_function_source
            .verify()
            .unwrap_err()
            .message
            .contains("function source reference is owned by")
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
    let helper_symbol = helper.artifact.symbol.as_str().to_owned();

    file.verify().unwrap();
    let syntax = crate::compiler::emit::emit_file(&file).unwrap();
    let rust = prettyplease::unparse(&syntax);

    assert!(rust.contains("fn main()"), "{rust}");
    assert!(
        rust.contains(&format!("pub fn {helper_symbol}()")),
        "{rust}"
    );
    assert!(!rust.contains("diagnostic_entry_name"), "{rust}");
    assert!(!rust.contains("diagnostic_helper_name"), "{rust}");
}

#[test]
fn idiom_pass_emits_straight_line_cfgs_without_a_pc_dispatch_loop() {
    let file = lower(
        "package main\nfunc identity(value int) int { return value }\nfunc main() { println(identity(3)) }\n",
    );

    assert!(file.functions.iter().all(|function| matches!(
        function.control_flow,
        ControlFlowPlan::StructuredLinear { .. }
    )));
    let syntax = crate::compiler::emit::emit_file(&file).unwrap();
    let rust = prettyplease::unparse(&syntax);
    assert!(!rust.contains("__gors_pc"), "{rust}");
    assert!(!rust.contains("invalid compiler Rust IR block"), "{rust}");
    assert!(rust.contains("return"), "{rust}");
}

#[test]
fn verifier_rejects_a_noncanonical_structured_block_order() {
    let mut file = lower("package main\nfunc identity(value int) int { return value }\n");
    let function = file.functions.first_mut().unwrap();
    let entry = function.entry;
    assert!(matches!(
        function.control_flow,
        ControlFlowPlan::StructuredLinear { .. }
    ));
    if let ControlFlowPlan::StructuredLinear { order } = &mut function.control_flow {
        order.push(entry);
    }

    let error = file.verify().unwrap_err();
    assert!(
        error.message.contains("block order is not canonical"),
        "{error:?}"
    );
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
    assert!(
        library_main
            .artifact
            .symbol
            .as_str()
            .starts_with("__gors_fn_")
    );
    assert_eq!(
        library_main.artifact,
        FunctionArtifactPlan::public_definition(library_main.id)
    );

    let syntax = crate::compiler::emit::emit_file(&library).unwrap();
    let rust = prettyplease::unparse(&syntax);
    assert!(
        rust.contains(&format!(
            "pub fn {}()",
            library_main.artifact.symbol.as_str()
        )),
        "{rust}"
    );
    assert!(!rust.contains("fn main()"), "{rust}");
}

fn all_rvalues(file: &File) -> impl Iterator<Item = &Rvalue> {
    file.functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.statements)
        .map(|statement| &statement.value)
}

fn runtime_operations_in_execution_order(file: &File, function_name: &str) -> Vec<RuntimeOp> {
    let function = file
        .functions
        .iter()
        .find(|function| function.name == function_name)
        .unwrap();
    let mut block = function.entry;
    let mut operations = Vec::new();
    let mut visited = 0usize;
    loop {
        visited = visited.saturating_add(1);
        assert!(
            visited <= function.blocks.len().saturating_add(1),
            "unexpected cycle while following lowered runtime calls"
        );
        let terminator = &function.blocks[block.0 as usize].terminator.kind;
        match terminator {
            TerminatorKind::Goto(next) => block = *next,
            TerminatorKind::Call {
                target: CallTarget::Runtime(operation),
                next,
                ..
            } => {
                operations.push(*operation);
                block = *next;
            }
            TerminatorKind::Return(_) => return operations,
            other => {
                assert!(
                    matches!(other, TerminatorKind::Return(_)),
                    "unexpected terminator while following lowered runtime calls: {other:?}"
                );
                return operations;
            }
        }
    }
}

fn binary_rvalue(file: &File, expected: ValueOp) -> &Rvalue {
    all_rvalues(file)
        .find(|rvalue| matches!(rvalue.kind, RvalueKind::Binary { op, .. } if op == expected))
        .unwrap()
}

fn binary_rvalue_mut(file: &mut File, expected: ValueOp) -> &mut Rvalue {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.statements)
        .map(|statement| &mut statement.value)
        .find(|rvalue| matches!(rvalue.kind, RvalueKind::Binary { op, .. } if op == expected))
        .unwrap()
}

fn binary_value_op_mut(file: &mut File, expected: ValueOp) -> &mut ValueOp {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.statements)
        .find_map(|statement| match &mut statement.value.kind {
            RvalueKind::Binary { op, .. } if *op == expected => Some(op),
            _ => None,
        })
        .unwrap()
}

fn runtime_call_target_mut(file: &mut File, expected: RuntimeOp) -> &mut RuntimeOp {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .find_map(|block| match &mut block.terminator.kind {
            TerminatorKind::Call {
                target: CallTarget::Runtime(operation),
                ..
            } if *operation == expected => Some(operation),
            _ => None,
        })
        .unwrap()
}

fn first_function_call_terminator_mut(file: &mut File) -> &mut Terminator {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .find_map(|block| {
            matches!(
                &block.terminator.kind,
                TerminatorKind::Call {
                    target: CallTarget::Function(_),
                    ..
                }
            )
            .then_some(&mut block.terminator)
        })
        .unwrap()
}

fn runtime_call_terminator_mut(file: &mut File, expected: RuntimeOp) -> &mut Terminator {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .find_map(|block| {
            matches!(
                &block.terminator.kind,
                TerminatorKind::Call {
                    target: CallTarget::Runtime(operation),
                    ..
                } if *operation == expected
            )
            .then_some(&mut block.terminator)
        })
        .unwrap()
}

fn static_bytes_runtime_op_mut(file: &mut File) -> &mut RuntimeOp {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.statements)
        .find_map(|statement| match &mut statement.value.kind {
            RvalueKind::Use(Operand::Constant(Constant::RuntimeStaticBytes { op, .. })) => Some(op),
            _ => None,
        })
        .unwrap()
}

fn read_op_mut<'a>(
    file: &'a mut File,
    function_name: &str,
    expected: ReadOp,
) -> Option<&'a mut ReadOp> {
    let function = file
        .functions
        .iter_mut()
        .find(|function| function.name == function_name)?;
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            if let Some(op) = rvalue_read_op_mut(&mut statement.value, expected) {
                return Some(op);
            }
        }
        if let Some(op) = terminator_read_op_mut(&mut block.terminator, expected) {
            return Some(op);
        }
    }
    None
}

fn rvalue_read_op_mut(rvalue: &mut Rvalue, expected: ReadOp) -> Option<&mut ReadOp> {
    match &mut rvalue.kind {
        RvalueKind::Use(operand) | RvalueKind::Unary { operand, .. } => {
            operand_read_op_mut(operand, expected)
        }
        RvalueKind::Binary { left, right, .. } => {
            operand_read_op_mut(left, expected).or_else(|| operand_read_op_mut(right, expected))
        }
    }
}

fn terminator_read_op_mut(terminator: &mut Terminator, expected: ReadOp) -> Option<&mut ReadOp> {
    match &mut terminator.kind {
        TerminatorKind::SwitchBool { condition, .. } => operand_read_op_mut(condition, expected),
        TerminatorKind::Call { args, .. } | TerminatorKind::Return(args) => args
            .iter_mut()
            .find_map(|argument| operand_read_op_mut(argument, expected)),
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => None,
    }
}

fn operand_read_op_mut(operand: &mut Operand, expected: ReadOp) -> Option<&mut ReadOp> {
    match operand {
        Operand::Read { op, .. } if *op == expected => Some(op),
        Operand::Read { .. } | Operand::Constant(_) | Operand::Unit => None,
    }
}

fn refresh_test_effects(file: &mut File) {
    for function in &mut file.functions {
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                let effects = rvalue_effects(&statement.value.kind);
                statement.value.effects = effects;
                statement.value.panic = panic_edge(effects);
                statement.effects = statement_effects(&statement.value);
            }
            let effects = terminator_effects(&block.terminator.kind);
            block.terminator.effects = effects;
            block.terminator.panic = panic_edge(effects);
        }
    }
}
