#![allow(clippy::enum_variant_names)]

mod hashable;
mod printable;
mod printer;

use crate::token::{Position, Token};
use std::collections::BTreeMap;

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
    pub func: Option<Position<'a>>, // position of "func" keyword (token.NoPos if there is no "func")
    pub params: &'a FieldList<'a>,  // (incoming) parameters; non-nil
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
    pub values: Option<Vec<Expr<'a>>>, // initial values; or nil
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

// https://pkg.go.dev/go/ast#Ellipsis
#[derive(Debug)]
pub struct Ellipsis<'a> {
    pub ellipsis: Position<'a>, // position of "..."
    pub elt: Expr<'a>,          // ellipsis element type (parameter lists only); or nil
}

// https://pkg.go.dev/go/ast#TypeSpec
#[derive(Debug)]
pub struct TypeSpec<'a> {
    pub doc: Option<&'a CommentGroup>, // associated documentation; or nil
    pub name: Option<&'a Ident<'a>>,   // type name
    pub assign: Option<Position<'a>>,  // position of '=', if any
    pub type_: Expr<'a>, // *Ident, *ParenExpr, *SelectorExpr, *StarExpr, or any of the *XxxTypes
    pub comment: Option<&'a CommentGroup>, // line comments; or nil
}

// https://pkg.go.dev/go/ast#StructType
#[derive(Debug)]
pub struct StructType<'a> {
    pub struct_: Position<'a>,             // position of "struct" keyword
    pub fields: Option<&'a FieldList<'a>>, // list of field declarations
    pub incomplete: bool,                  // true if (source) fields are missing in the Fields list
}

// https://pkg.go.dev/go/ast#StarExpr
#[derive(Debug)]
pub struct StarExpr<'a> {
    pub star: Position<'a>, // position of "*"
    pub x: Expr<'a>,        // operand
}

// https://pkg.go.dev/go/ast#InterfaceType
#[derive(Debug)]
pub struct InterfaceType<'a> {
    pub interface: Position<'a>,            // position of "interface" keyword
    pub methods: Option<&'a FieldList<'a>>, // list of embedded interfaces, methods, or types
    pub incomplete: bool, // true if (source) methods or types are missing in the Methods list
}

// https://pkg.go.dev/go/ast#DeclStmt
#[derive(Debug)]
pub struct DeclStmt<'a> {
    pub decl: &'a GenDecl<'a>, // *GenDecl with CONST, TYPE, or VAR token
}

// https://pkg.go.dev/go/ast#UnaryExpr
#[derive(Debug)]
pub struct UnaryExpr<'a> {
    pub op_pos: Position<'a>, // position of Op
    pub op: Token,            // operator
    pub x: Expr<'a>,          // operand
}

// https://pkg.go.dev/go/ast#CallExpr
#[derive(Debug)]
pub struct CallExpr<'a> {
    pub fun: Expr<'a>,                  // function expression
    pub lparen: Position<'a>,           // position of "("
    pub args: Option<Vec<Expr<'a>>>,    // function arguments; or nil
    pub ellipsis: Option<Position<'a>>, // position of "..." (token.NoPos if there is no "...")
    pub rparen: Position<'a>,           // position of ")"
}

// https://pkg.go.dev/go/ast#SelectorExpr
#[derive(Debug)]
pub struct SelectorExpr<'a> {
    pub x: Expr<'a>,        // expression
    pub sel: &'a Ident<'a>, // field selector
}

// https://pkg.go.dev/go/ast#ExprStmt
#[derive(Debug)]
pub struct ExprStmt<'a> {
    pub x: Expr<'a>, // expression
}

// https://pkg.go.dev/go/ast#SelectorExpr
#[derive(Debug)]
pub struct IfStmt<'a> {
    pub if_: Position<'a>,      // position of "if" keyword
    pub init: Option<Stmt<'a>>, // initialization statement; or nil
    pub cond: Expr<'a>,         // condition
    pub body: &'a BlockStmt<'a>,
    pub else_: Option<Stmt<'a>>, // else branch; or nil
}

// https://pkg.go.dev/go/ast#Spec
#[derive(Debug, Copy, Clone)]
pub enum Spec<'a> {
    ImportSpec(&'a ImportSpec<'a>),
    TypeSpec(&'a TypeSpec<'a>),
    ValueSpec(&'a ValueSpec<'a>),
}

// https://pkg.go.dev/go/ast#Expr
#[derive(Debug, Copy, Clone)]
pub enum Expr<'a> {
    BasicLit(&'a BasicLit<'a>),
    BinaryExpr(&'a BinaryExpr<'a>),
    CallExpr(&'a CallExpr<'a>),
    Ellipsis(&'a Ellipsis<'a>),
    FuncType(&'a FuncType<'a>),
    Ident(&'a Ident<'a>),
    InterfaceType(&'a InterfaceType<'a>),
    SelectorExpr(&'a SelectorExpr<'a>),
    StarExpr(&'a StarExpr<'a>),
    StructType(&'a StructType<'a>),
    UnaryExpr(&'a UnaryExpr<'a>),
}

// https://pkg.go.dev/go/ast#Stmt
#[derive(Debug, Copy, Clone)]
pub enum Stmt<'a> {
    AssignStmt(&'a AssignStmt<'a>),
    BlockStmt(&'a BlockStmt<'a>),
    DeclStmt(&'a DeclStmt<'a>),
    ExprStmt(&'a ExprStmt<'a>),
    IfStmt(&'a IfStmt<'a>),
    ReturnStmt(&'a ReturnStmt<'a>),
}
