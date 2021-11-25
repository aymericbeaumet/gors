mod hashable;
mod printable;
mod printer;
mod resolver;
mod visitable;
mod visitor;

use crate::token::{Position, Token};
use std::collections::BTreeMap;

pub use resolver::Resolver;
pub use visitor::{Visitable, Visitor};

pub fn fprint<W: std::io::Write, T: printer::Printable<W>>(
    w: W,
    node: T,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut p = printer::Printer::new(w);
    node.print(&mut p)
}

// https://pkg.go.dev/go/ast#CommentGroup
#[derive(Debug)]
pub struct CommentGroup {
    // List []*Comment // len(List) > 0
}

// https://pkg.go.dev/go/ast#FieldList
#[derive(Debug)]
pub struct FieldList<'a> {
    pub opening: Option<Position<'a>>, // position of opening parenthesis/brace, if any
    pub list: Vec<&'a Field<'a>>,      // field list; or nil
    pub closing: Option<Position<'a>>, // position of closing parenthesis/brace, if any
}

// https://pkg.go.dev/go/ast#Field
#[derive(Debug)]
pub struct Field<'a> {
    pub doc: Option<&'a CommentGroup>, // associated documentation; or nil
    pub names: Option<Vec<&'a Ident<'a>>>, // field/method/(type) parameter names, or type "type"; or nil
    pub type_: Option<Expr<'a>>,           // field/method/parameter type, type list type; or nil
    pub tag: Option<&'a BasicLit<'a>>,     // field tag; or nil
    pub comment: Option<&'a CommentGroup>, // line comments; or nil
}

// https://pkg.go.dev/go/ast#File
#[derive(Debug)]
pub struct File<'a> {
    pub doc: Option<&'a CommentGroup>, // associated documentation; or nil
    pub package: Position<'a>,         // position of "package" keyword
    pub name: &'a Ident<'a>,           // package name
    pub decls: Vec<Decl<'a>>,          // top-level declarations; or nil
    pub scope: Option<&'a Scope<'a>>,  // package scope (this file only)
    pub imports: Vec<&'a ImportSpec<'a>>, // imports in this file
    pub unresolved: Vec<&'a Ident<'a>>, // unresolved identifiers in this file
    pub comments: Vec<&'a CommentGroup>, // list of all comments in the source file
}

// https://pkg.go.dev/go/ast#FuncDecl
#[derive(Debug)]
pub struct FuncDecl<'a> {
    pub doc: Option<&'a CommentGroup>, // associated documentation; or nil
    pub recv: Option<&'a FieldList<'a>>, // receiver (methods); or nil (functions)
    pub name: &'a Ident<'a>,           // function/method name
    pub type_: &'a FuncType<'a>, // function signature: type and value parameters, results, and position of "func" keyword
    pub body: Option<&'a BlockStmt<'a>>, // function body; or nil for external (non-Go) function
}

// https://pkg.go.dev/go/ast#BlockStmt
#[derive(Debug)]
pub struct BlockStmt<'a> {
    pub lbrace: Position<'a>, // position of "{"
    pub list: Vec<Stmt<'a>>,
    pub rbrace: Position<'a>, // position of "}", if any (may be absent due to syntax error)
}

// https://pkg.go.dev/go/ast#FuncType
#[derive(Debug)]
pub struct FuncType<'a> {
    pub func: Position<'a>, // position of "func" keyword (token.NoPos if there is no "func")
    pub params: &'a FieldList<'a>, // (incoming) parameters; non-nil
    pub results: Option<&'a FieldList<'a>>, // (outgoing) results; or nil
}

// https://pkg.go.dev/go/ast#Ident
#[derive(Debug)]
pub struct Ident<'a> {
    pub name_pos: Position<'a>,                       // identifier position
    pub name: &'a str,                                // identifier name
    pub obj: std::cell::Cell<Option<&'a Object<'a>>>, // denoted object; or nil
}

// https://pkg.go.dev/go/ast#ImportSpec
#[derive(Debug)]
pub struct ImportSpec<'a> {
    pub doc: Option<&'a CommentGroup>, // associated documentation; or nil
    pub name: Option<&'a Ident<'a>>,   // local package name (including "."); or nil
    pub path: &'a BasicLit<'a>,        // import path
    pub comment: Option<&'a CommentGroup>, // line comments; or nil
                                       //pub end_pos: Position<'a>,         // end of spec (overrides Path.Pos if nonzero)
}

// https://pkg.go.dev/go/ast#ValueSpec
#[derive(Debug)]
pub struct ValueSpec<'a> {
    pub doc: Option<&'a CommentGroup>, // associated documentation; or nil
    pub names: Vec<&'a Ident<'a>>,     // value names (len(Names) > 0)
    pub type_: Option<Expr<'a>>,       // value type; or nil
    pub values: Vec<Expr<'a>>,         // initial values; or nil
    pub comment: Option<&'a CommentGroup>, // line comments; or nil
}

// https://pkg.go.dev/go/ast#BasicLit
#[derive(Debug)]
pub struct BasicLit<'a> {
    pub value_pos: Position<'a>, // literal position
    pub kind: Token,             // token.INT, token.FLOAT, token.IMAG, token.CHAR, or token.STRING
    pub value: &'a str, // literal string; e.g. 42, 0x7f, 3.14, 1e-9, 2.4i, 'a', '\x7f', "foo" or `\m\n\o`
}

// https://pkg.go.dev/go/ast#Object
#[derive(Debug)]
pub struct Object<'a> {
    pub kind: ObjKind,
    pub name: &'a str,             // declared name
    pub decl: Option<ObjDecl<'a>>, // corresponding Field, XxxSpec, FuncDecl, LabeledStmt, AssignStmt, Scope; or nil
    pub data: Option<usize>,       // object-specific data; or nil
    pub type_: Option<()>,         // placeholder for type information; may be nil
}

// https://pkg.go.dev/go/ast#ObjKind
#[derive(Debug)]
pub enum ObjKind {
    //Pkg, // package
    Con, // constant
    //Typ, // type
    Var, // variable
    Fun, // function or method
         //Lbl, // label
}

#[derive(Debug)]
pub enum ObjDecl<'a> {
    FuncDecl(&'a FuncDecl<'a>),
    ValueSpec(&'a ValueSpec<'a>),
}

// https://pkg.go.dev/go/ast#Decl
#[derive(Debug)]
pub enum Decl<'a> {
    FuncDecl(&'a FuncDecl<'a>),
    GenDecl(&'a GenDecl<'a>),
}

// https://pkg.go.dev/go/ast#Scope
#[derive(Debug)]
pub struct Scope<'a> {
    pub outer: Option<&'a Scope<'a>>,
    pub objects: BTreeMap<&'a str, &'a Object<'a>>,
}

// https://pkg.go.dev/go/ast#GenDecl
#[derive(Debug)]
pub struct GenDecl<'a> {
    pub doc: Option<&'a CommentGroup>, // associated documentation; or nil
    pub tok_pos: Position<'a>,         // position of Tok
    pub tok: Token,                    // IMPORT, CONST, TYPE, or VAR
    pub lparen: Option<Position<'a>>,  // position of '(', if any
    pub specs: Vec<Spec<'a>>,
    pub rparen: Option<Position<'a>>, // position of ')', if any
}

// https://pkg.go.dev/go/ast#AssignStmt
#[derive(Debug)]
pub struct AssignStmt<'a> {
    pub lhs: Vec<Expr<'a>>,
    pub tok_pos: Position<'a>, // position of Tok
    pub tok: Token,            // assignment token, DEFINE
    pub rhs: Vec<Expr<'a>>,
}

// https://pkg.go.dev/go/ast#BinaryExpr
#[derive(Debug)]
pub struct BinaryExpr<'a> {
    pub x: Expr<'a>,          // left operand
    pub op_pos: Position<'a>, // position of Op
    pub op: Token,            // operator
    pub y: Expr<'a>,          // right operand
}

// https://pkg.go.dev/go/ast#ReturnStmt
#[derive(Debug)]
pub struct ReturnStmt<'a> {
    pub return_: Position<'a>,  // position of "return" keyword
    pub results: Vec<Expr<'a>>, // result expressions; or nil
}

// https://pkg.go.dev/go/ast#Spec
#[derive(Debug)]
pub enum Spec<'a> {
    ImportSpec(&'a ImportSpec<'a>),
    ValueSpec(&'a ValueSpec<'a>),
}

// https://pkg.go.dev/go/ast#Expr
#[derive(Debug)]
pub enum Expr<'a> {
    BasicLit(&'a BasicLit<'a>),
    BinaryExpr(&'a BinaryExpr<'a>),
    Ident(&'a Ident<'a>),
}

// https://pkg.go.dev/go/ast#Stmt
#[derive(Debug)]
pub enum Stmt<'a> {
    AssignStmt(&'a AssignStmt<'a>),
    ReturnStmt(&'a ReturnStmt<'a>),
}
