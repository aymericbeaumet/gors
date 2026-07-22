use super::*;
use num_bigint::BigInt;

fn lower(source: &str) -> Result<hir::File, Vec<Diagnostic>> {
    let parsed = crate::parser::parse_file("semantic.go", source).expect("valid Go syntax");
    lower_file(&parsed)
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
