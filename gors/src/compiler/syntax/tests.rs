use crate::ast;
use crate::parser::parse_file;

use super::{
    ExprSyntaxKind, ProjectedFunctionSyntax, StmtSyntaxKind, SyntaxAnchor, project_function,
};

fn project(source: &str, name: &str) -> ProjectedFunctionSyntax {
    let parsed = parse_file("syntax_projection.go", source).expect("source should parse");
    let function = parsed
        .ast()
        .decls
        .iter()
        .find_map(|declaration| match declaration {
            ast::Decl::FuncDecl(function) if function.name.name == name => Some(function),
            _ => None,
        })
        .expect("function should exist");
    project_function(function, parsed.token_observations()).expect("projection should succeed")
}

#[test]
fn function_anchor_is_independent_of_layout_and_declaration_order() {
    let first = project("package p\nfunc f() {}\nfunc g() {}\n", "f");
    let reordered = project(
        "package p\n\nfunc g() {}\n\n\nfunc f () { /* trivia */ }\n",
        "f",
    );

    assert_eq!(first.anchor, reordered.anchor);
    assert_eq!(first.anchor, SyntaxAnchor::named_function("f"));
    assert_ne!(first.anchor, SyntaxAnchor::named_function("g"));
    assert_ne!(first.layout, reordered.layout);
}

#[test]
fn trivia_and_semicolon_spelling_do_not_change_owned_function_syntax() {
    let inserted = project(
        concat!(
            "package p\n",
            "func f(value int) int {\n",
            "\tvalue = value + 1\n",
            "\treturn value\n",
            "}\n",
        ),
        "f",
    );
    let explicit = project(
        concat!(
            "package p\n\n",
            "// declaration trivia\n",
            "func /* header */ f ( value int ) int /* brace */ {\n",
            "\tvalue=value+1; // statement trivia\n",
            "\treturn value;\n",
            "}\n",
        ),
        "f",
    );

    assert_eq!(inserted.anchor, explicit.anchor);
    assert_eq!(inserted.header, explicit.header);
    assert_eq!(inserted.body, explicit.body);
    assert_eq!(inserted.header.fingerprint(), explicit.header.fingerprint());
    assert_eq!(
        inserted
            .body
            .as_ref()
            .expect("body should exist")
            .fingerprint(),
        explicit
            .body
            .as_ref()
            .expect("body should exist")
            .fingerprint(),
    );
    assert_ne!(inserted.layout, explicit.layout);
}

#[test]
fn header_and_body_edits_are_separate_semantic_products() {
    let original = project("package p\nfunc f(value int) int { return value }\n", "f");
    let body_edit = project(
        "package p\nfunc f(value int) int { return value + 1 }\n",
        "f",
    );
    let header_edit = project(
        "package p\nfunc f(value int, other int) int { return value }\n",
        "f",
    );

    assert_eq!(original.header, body_edit.header);
    assert_ne!(original.body, body_edit.body);
    assert_ne!(original.header, header_edit.header);
    assert_eq!(original.body, header_edit.body);
}

#[test]
fn bodyless_header_uses_existing_semicolon_observation() {
    let inserted = project("package p\nfunc f(value int) int\n", "f");
    let explicit = project("package p\nfunc f(value int) int;\n", "f");

    assert_eq!(inserted.header, explicit.header);
    assert!(inserted.body.is_none());
    assert!(explicit.body.is_none());
    assert_ne!(inserted.layout, explicit.layout);
}

#[test]
fn selector_projection_preserves_chains_and_distinct_sources() {
    let projected = project("package p\nfunc f() { use(client.API.Call) }\n", "f");
    let block = projected
        .structural_body
        .block
        .as_ref()
        .expect("function should have a body");
    let [statement] = block.statements.as_ref() else {
        panic!("function body should contain one statement");
    };
    let StmtSyntaxKind::Expr(call) = &statement.kind else {
        panic!("function body should contain an expression statement");
    };
    let ExprSyntaxKind::Call { arguments, .. } = &call.kind else {
        panic!("expression statement should contain a call");
    };
    let [selector] = arguments.as_ref() else {
        panic!("call should have one selector argument");
    };
    let ExprSyntaxKind::Selector { base, member } = &selector.kind else {
        panic!("argument should retain its outer selector");
    };
    assert_eq!(member.name.as_ref(), "Call");
    assert_ne!(base.source, member.source);
    assert_ne!(
        projected.layout.resolve(base.source).unwrap(),
        projected.layout.resolve(member.source).unwrap()
    );

    let ExprSyntaxKind::Selector {
        base: root,
        member: intermediate,
    } = &base.kind
    else {
        panic!("selector base should retain its inner selector");
    };
    assert_eq!(intermediate.name.as_ref(), "API");
    assert_ne!(root.source, intermediate.source);
    let ExprSyntaxKind::Ident(root) = &root.kind else {
        panic!("inner selector should retain its identifier base");
    };
    assert_eq!(root.name.as_ref(), "client");

    let sources = [
        selector.source,
        member.source,
        base.source,
        intermediate.source,
        root.source,
    ];
    for (index, source) in sources.iter().enumerate() {
        assert!(
            !sources.get(..index).unwrap_or_default().contains(source),
            "each structural selector component should own a distinct source"
        );
    }
}

#[test]
fn selector_trivia_is_stable_but_member_edits_change_body_fingerprint() {
    let compact = project("package p\nfunc f() { use(client.API.Call) }\n", "f");
    let spaced = project(
        "package p\nfunc f () { use ( client /* base */ . API . Call ) }\n",
        "f",
    );
    let edited = project("package p\nfunc f() { use(client.API.Fetch) }\n", "f");

    assert_eq!(compact.structural_body, spaced.structural_body);
    assert_eq!(
        compact.body.as_ref().unwrap().fingerprint(),
        spaced.body.as_ref().unwrap().fingerprint()
    );
    assert_ne!(compact.layout, spaced.layout);
    assert_ne!(compact.structural_body, edited.structural_body);
    assert_ne!(
        compact.body.as_ref().unwrap().fingerprint(),
        edited.body.as_ref().unwrap().fingerprint()
    );
}
