use crate::ast;
use crate::parser::parse_file;

use super::{ProjectedFunctionSyntax, SyntaxAnchor, project_function};

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
