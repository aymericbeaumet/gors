use super::*;
use num_bigint::BigInt;
use std::collections::BTreeSet;

fn lower(source: &str) -> Result<hir::File, Vec<Diagnostic>> {
    lower_at("semantic.go", source)
}

fn lower_at(filename: &str, source: &str) -> Result<hir::File, Vec<Diagnostic>> {
    let parsed = crate::parser::parse_file(filename, source).expect("valid Go syntax");
    lower_file(parsed.ast())
}

fn function<'a>(file: &'a hir::File, name: &str) -> &'a hir::Function {
    file.functions
        .iter()
        .find(|function| function.name == name)
        .expect("named HIR function")
}

fn function_node_ids(function: &hir::Function) -> Vec<NodeId> {
    let mut nodes = vec![function.node];
    collect_block_nodes(&function.body, &mut nodes);
    nodes
}

fn collect_block_nodes(block: &hir::Block, nodes: &mut Vec<NodeId>) {
    nodes.push(block.node);
    for statement in &block.stmts {
        collect_statement_nodes(statement, nodes);
    }
}

fn collect_statement_nodes(statement: &hir::Stmt, nodes: &mut Vec<NodeId>) {
    nodes.push(statement.node);
    match &statement.kind {
        hir::StmtKind::Let { values, .. } | hir::StmtKind::Assign { values, .. } => {
            for expression in values {
                collect_expression_nodes(expression, nodes);
            }
        }
        hir::StmtKind::Expr(expression) => collect_expression_nodes(expression, nodes),
        hir::StmtKind::Return(expressions) => {
            for expression in expressions {
                collect_expression_nodes(expression, nodes);
            }
        }
        hir::StmtKind::If {
            init,
            condition,
            then_block,
            else_branch,
        } => {
            if let Some(init) = init {
                collect_statement_nodes(init, nodes);
            }
            collect_expression_nodes(condition, nodes);
            collect_block_nodes(then_block, nodes);
            if let Some(else_branch) = else_branch {
                collect_statement_nodes(else_branch, nodes);
            }
        }
        hir::StmtKind::For {
            init,
            condition,
            post,
            body,
        } => {
            if let Some(init) = init {
                collect_statement_nodes(init, nodes);
            }
            if let Some(condition) = condition {
                collect_expression_nodes(condition, nodes);
            }
            if let Some(post) = post {
                collect_statement_nodes(post, nodes);
            }
            collect_block_nodes(body, nodes);
        }
        hir::StmtKind::Block(block) => collect_block_nodes(block, nodes),
        hir::StmtKind::Break | hir::StmtKind::Continue => {}
    }
}

fn collect_expression_nodes(expression: &hir::Expr, nodes: &mut Vec<NodeId>) {
    nodes.push(expression.node);
    match &expression.kind {
        hir::ExprKind::Binary { left, right, .. } => {
            collect_expression_nodes(left, nodes);
            collect_expression_nodes(right, nodes);
        }
        hir::ExprKind::Unary { operand, .. } => collect_expression_nodes(operand, nodes),
        hir::ExprKind::Call { args, .. } => {
            for argument in args {
                collect_expression_nodes(argument, nodes);
            }
        }
        hir::ExprKind::Constant(_)
        | hir::ExprKind::Local(_)
        | hir::ExprKind::GlobalConstant(_, _) => {}
    }
}

#[test]
fn stable_definitions_and_owner_local_nodes_ignore_unrelated_declaration_order() {
    let original = lower_at(
        "main.go",
        r#"package main
            const answer = 42
            func helper(value int) int {
                println(1)
                println(1)
                return value + answer
            }
            func main() { println(helper(answer)) }
        "#,
    )
    .expect("original HIR");
    let reordered = lower_at(
        "main.go",
        r#"package main
            func unrelated() int { return 7 }
            func main() { println(helper(answer)) }
            const extra = 9
            func helper(value int) int {
                println(1)
                println(1)
                return value + answer
            }
            const answer = 42
        "#,
    )
    .expect("reordered HIR");

    for name in ["helper", "main"] {
        let original = function(&original, name);
        let reordered = function(&reordered, name);
        assert_eq!(original.id, reordered.id, "unstable DefId for {name}");
        assert_eq!(
            function_node_ids(original),
            function_node_ids(reordered),
            "another declaration perturbed owner-local nodes for {name}"
        );
    }
    let original_answer = original
        .constants
        .iter()
        .find(|constant| constant.name == "answer")
        .unwrap();
    let reordered_answer = reordered
        .constants
        .iter()
        .find(|constant| constant.name == "answer")
        .unwrap();
    assert_eq!(original_answer.id, reordered_answer.id);
}

#[test]
fn owner_local_node_ids_are_unique_for_identical_source_subtrees() {
    let file = lower(
        r#"package main
            func main() {
                println(1, 1)
                println(1, 1)
            }
        "#,
    )
    .expect("HIR with repeated syntax");
    let nodes = function_node_ids(function(&file, "main"));
    let unique = nodes.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(nodes.len(), unique.len(), "HIR node occurrences aliased");
}

#[test]
fn standalone_ids_do_not_embed_checkout_paths() {
    let source =
        "package main\nfunc helper() int { return 42 }\nfunc main() { println(helper()) }\n";
    let first = crate::parser::parse_file("/one/checkout/main.go", source).unwrap();
    let second = crate::parser::parse_file("/different/root/main.go", source).unwrap();
    let windows = crate::parser::parse_file(r"C:\different\root\main.go", source).unwrap();
    let first_hir = lower_file(first.ast()).unwrap();
    let second_hir = lower_file(second.ast()).unwrap();
    let windows_hir = lower_file(windows.ast()).unwrap();
    assert_eq!(
        function(&first_hir, "helper").id,
        function(&second_hir, "helper").id
    );
    assert_eq!(
        function(&first_hir, "helper").id,
        function(&windows_hir, "helper").id
    );
}

#[test]
fn canonical_import_paths_isolate_definition_ids_and_rust_symbols() {
    let source =
        "package shared\nfunc helper() int { return 42 }\nfunc main() { println(helper()) }\n";
    let parsed = crate::parser::parse_file("main.go", source).unwrap();
    let workspace = WorkspaceKey::AdHoc("workspace".into());
    let first_package = PackageKey::ImportPath("example/one".into());
    let second_package = PackageKey::ImportPath("example/two".into());
    let first_context = semantic_context(&workspace, &first_package, "main.go").unwrap();
    let second_context = semantic_context(&workspace, &second_package, "main.go").unwrap();
    let first = lower_file_with_context(parsed.ast(), first_context)
        .unwrap()
        .file;
    let second = lower_file_with_context(parsed.ast(), second_context)
        .unwrap()
        .file;
    let first_id = function(&first, "helper").id;
    let second_id = function(&second, "helper").id;
    assert_ne!(first_id, second_id);

    let first_symbol =
        crate::compiler::lower_to_rust_ir(crate::compiler::lower_to_mir(&first).unwrap())
            .unwrap()
            .as_file()
            .functions
            .iter()
            .find(|function| function.name == "helper")
            .unwrap()
            .artifact
            .symbol
            .as_str()
            .to_string();
    let second_symbol =
        crate::compiler::lower_to_rust_ir(crate::compiler::lower_to_mir(&second).unwrap())
            .unwrap()
            .as_file()
            .functions
            .iter()
            .find(|function| function.name == "helper")
            .unwrap()
            .artifact
            .symbol
            .as_str()
            .to_string();
    assert_ne!(first_symbol, second_symbol);
}

fn assert_diagnostic(source: &str, code: &str, message: &str) {
    let diagnostics = lower(source).expect_err("semantic lowering should reject source");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == code && diagnostic.message.contains(message)),
        "missing {code} containing {message:?}: {diagnostics:#?}"
    );
}

#[test]
fn rejects_local_const_instead_of_lowering_a_mutable_binding() {
    assert_diagnostic(
        "package main\nfunc main() { const answer = 42; println(answer) }",
        "GORS2001",
        "local const declarations require immutable HIR bindings",
    );
}

#[test]
fn rejects_numeric_representations_outside_the_bootstrap_frontier() {
    for source in [
        "package main\nfunc f(value int8) {}",
        "package main\nfunc f(value uint) {}",
        "package main\nfunc f(value float64) {}",
        "package main\nfunc main() { println(1e1000) }",
        "package main\nconst value = -1.25\nfunc main() {}",
    ] {
        assert_diagnostic(
            source,
            "GORS2001",
            "bootstrap bool/int/string runtime frontier",
        );
    }
}

#[test]
fn rejects_constants_that_do_not_fit_target_int() {
    let out_of_range = BigInt::from(i64::MAX) + 1;
    assert_diagnostic(
        &format!("package main\nfunc main() {{ var value int = {out_of_range}; println(value) }}"),
        "GORS2002",
        "not representable",
    );
    assert_diagnostic(
        &format!(
            "package main\nfunc main() {{ value := {} + 1; println(value) }}",
            i64::MAX
        ),
        "GORS2002",
        "not representable",
    );
    lower(&format!(
        "package main\nfunc main() {{ println({}) }}",
        i64::MIN
    ))
    .expect("the target int minimum is representable after exact unary folding");
    assert_diagnostic(
        "package main\nfunc main() { println(1 / 0) }",
        "GORS2002",
        "division by zero",
    );
    lower("package main\nconst answer = 40 + 2\nfunc main() { println(answer) }")
        .expect("top-level constants use the same exact evaluator");
}

#[test]
fn validates_operator_type_pairs() {
    for (source, message) in [
        (
            "package main\nfunc main() { println(true < false) }",
            "operator Less is invalid for Bool",
        ),
        (
            "package main\nfunc main() { println(true + false) }",
            "operator Add is invalid for Bool",
        ),
        (
            "package main\nfunc main() { println(\"a\" % \"b\") }",
            "operator Rem is invalid for String",
        ),
        (
            "package main\nfunc main() { value := true; value++ }",
            "increment and decrement require an int operand",
        ),
    ] {
        assert_diagnostic(source, "GORS2002", message);
    }

    lower("package main\nfunc main() { println(true == false, \"a\" < \"b\", 7 & 3) }")
        .expect("valid bootstrap operators");
}

#[test]
fn lexical_bindings_and_functions_shadow_predeclared_names() {
    let file = lower(
        r#"package main
            func print(value int) int { return value }
            func main() {
                true := 7
                println(print(true))
            }"#,
    )
    .expect("shadowing predeclared names is valid");
    let main = file
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main HIR");
    let hir::StmtKind::Expr(hir::Expr {
        kind: hir::ExprKind::Call { callee, args },
        ..
    }) = &main.body.stmts.get(1).expect("println statement").kind
    else {
        panic!("expected println call")
    };
    assert_eq!(*callee, hir::Callee::Builtin(hir::Builtin::Println));
    assert!(matches!(
        args.as_slice(),
        [hir::Expr {
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Function(_),
                ..
            },
            ..
        }]
    ));

    assert_diagnostic(
        "package main\nfunc main() { println := 1; println(println) }",
        "GORS2001",
        "calling local value println",
    );
    assert_diagnostic(
        "package main\nfunc f() {}\nfunc main() { value := f; println(value) }",
        "GORS2001",
        "function value f",
    );
}

#[test]
fn grouped_var_specs_have_go_scope_and_initialization_order() {
    let file = lower(
        r#"package main
            func main() {
                var (
                    first = 1
                    second = first
                )
                println(second)
            }"#,
    )
    .expect("earlier grouped var spec is visible to a later spec");
    let main = file.functions.first().expect("main HIR");
    let hir::StmtKind::Block(group) = &main.body.stmts.first().expect("grouped declaration").kind
    else {
        panic!("grouped var declaration must retain sequential ValueSpecs")
    };
    assert_eq!(group.stmts.len(), 2);
    let hir::StmtKind::Let {
        destinations: first_destinations,
        ..
    } = &group.stmts.first().expect("first ValueSpec").kind
    else {
        panic!("first ValueSpec must be a let")
    };
    let hir::Place::Local(first) = first_destinations.first().expect("first destination") else {
        panic!("first binding must be local")
    };
    let first = *first;
    let hir::StmtKind::Let { values, .. } = &group.stmts.get(1).expect("second ValueSpec").kind
    else {
        panic!("second ValueSpec must be a let")
    };
    assert!(matches!(
        values.first().expect("second initializer").kind,
        hir::ExprKind::Local(local) if local == first
    ));

    assert_diagnostic(
        "package main\nfunc main() { var first, second = 1, first; println(second) }",
        "GORS2002",
        "undefined identifier first",
    );
}

#[test]
fn expression_arity_never_degrades_to_unit_or_tuple_values() {
    assert_diagnostic(
        "package main\nfunc empty() {}\nfunc main() { value := empty(); println(value) }",
        "GORS2001",
        "no-result call cannot be used as a value",
    );
    assert_diagnostic(
        "package main\nfunc main() { value := println(1); println(value) }",
        "GORS2001",
        "no-result call cannot be used as a value",
    );
    assert_diagnostic(
        "package main\nfunc pair() (int, int) { return 1, 2 }\nfunc main() {}",
        "GORS2001",
        "multiple-result functions require explicit expression-arity HIR",
    );
    lower("package main\nfunc empty() {}\nfunc main() { empty(); println(1) }")
        .expect("discarded call results are valid expression statements");
}

#[test]
fn rejects_duplicate_short_declaration_names_and_accepts_blank_assignment() {
    assert_diagnostic(
        "package main\nfunc main() { first, first := 1, 2; println(first) }",
        "GORS2002",
        "appears more than once on the left of :=",
    );
    lower("package main\nfunc main() { _ = 1; _, _ = 2, 3 }")
        .expect("blank assignments evaluate and discard their RHS values");
}

#[test]
fn validates_special_function_boundaries() {
    assert_diagnostic(
        "package main\nfunc main(value int) {}",
        "GORS2002",
        "func main must have no parameters and no results",
    );
    assert_diagnostic(
        "package main\nfunc init() {}\nfunc main() {}",
        "GORS2001",
        "package init functions are not implemented",
    );
}

#[test]
fn semantic_diagnostics_retain_physical_anchors_across_line_directives() {
    let source = "package main\n//line generated.go:40\nfunc (value int) method() {}\n";
    let diagnostics = lower_at("/workspace/main.go", source).unwrap_err();
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("methods are not implemented"))
        .expect("unsupported method diagnostic");

    let crate::compiler::diagnostic::DiagnosticLocation::Physical(range) = diagnostic.location
    else {
        panic!("semantic diagnostic must own a physical file range");
    };
    assert_eq!(
        range.range().start().to_usize(),
        source.find("method").unwrap()
    );
}
