#![allow(clippy::large_enum_variant)] // TODO: we allow large enum variant for now, let's profile properly to see if we want to box.

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
    pub list: Vec<Field<'a>>,          // field list; or nil
    pub closing: Option<Position<'a>>, // position of closing parenthesis/brace, if any
}

// https://pkg.go.dev/go/ast#Field
#[derive(Debug)]
pub struct Field<'a> {
    pub doc: Option<CommentGroup>,     // associated documentation; or nil
    pub names: Option<Vec<Ident<'a>>>, // field/method/(type) parameter names, or type "type"; or nil
    pub type_: Option<Expr<'a>>,       // field/method/parameter type, type list type; or nil
    pub tag: Option<BasicLit<'a>>,     // field tag; or nil
    pub comment: Option<CommentGroup>, // line comments; or nil
}

// https://pkg.go.dev/go/ast#File
#[derive(Debug)]
pub struct File<'a> {
    pub doc: Option<CommentGroup>, // associated documentation; or nil
    pub package: Position<'a>,     // position of "package" keyword
    pub name: Ident<'a>,           // package name
    pub decls: Vec<Decl<'a>>,      // top-level declarations; or nil
    pub scope: Option<Scope<'a>>,  // package scope (this file only)
    //pub imports: Vec<&'a ImportSpec<'a>>, // imports in this file
    pub unresolved: Vec<Ident<'a>>, // unresolved identifiers in this file
    pub comments: Vec<CommentGroup>, // list of all comments in the source file
}

impl<'a> File<'a> {
    pub fn imports(&self) -> Vec<&ImportSpec<'a>> {
        self.decls
            .iter()
            .filter_map(|decl| {
                if let Decl::GenDecl(decl) = decl {
                    if decl.tok == Token::IMPORT {
                        return Some(decl.specs.iter());
                    }
                }
                None
            })
            .flatten()
            .filter_map(|spec| {
                if let Spec::ImportSpec(spec) = spec {
                    return Some(spec);
                }
                None
            })
            .collect()
    }
}

// https://pkg.go.dev/go/ast#FuncDecl
#[derive(Debug)]
pub struct FuncDecl<'a> {
    pub doc: Option<CommentGroup>,   // associated documentation; or nil
    pub recv: Option<FieldList<'a>>, // receiver (methods); or nil (functions)
    pub name: Ident<'a>,             // function/method name
    pub type_: FuncType<'a>, // function signature: type and value parameters, results, and position of "func" keyword
    pub body: Option<BlockStmt<'a>>, // function body; or nil for external (non-Go) function
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
    pub params: FieldList<'a>,      // (incoming) parameters; non-nil
    pub results: Option<FieldList<'a>>, // (outgoing) results; or nil
}

// https://pkg.go.dev/go/ast#Ident
#[derive(Debug)]
pub struct Ident<'a> {
    pub name_pos: Position<'a>,       // identifier position
    pub name: &'a str,                // identifier name
    pub obj: Option<Box<Object<'a>>>, // denoted object; or nil
}

// https://pkg.go.dev/go/ast#ImportSpec
#[derive(Debug)]
pub struct ImportSpec<'a> {
    pub doc: Option<CommentGroup>, // associated documentation; or nil
    pub name: Option<Ident<'a>>,   // local package name (including "."); or nil
    pub path: BasicLit<'a>,        // import path
    pub comment: Option<CommentGroup>, // line comments; or nil
                                   //pub end_pos: Position<'a>,         // end of spec (overrides Path.Pos if nonzero)
}

// https://pkg.go.dev/go/ast#ValueSpec
#[derive(Debug)]
pub struct ValueSpec<'a> {
    pub doc: Option<CommentGroup>,     // associated documentation; or nil
    pub names: Vec<Ident<'a>>,         // value names (len(Names) > 0)
    pub type_: Option<Expr<'a>>,       // value type; or nil
    pub values: Option<Vec<Expr<'a>>>, // initial values; or nil
    pub comment: Option<CommentGroup>, // line comments; or nil
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
    FuncDecl(FuncDecl<'a>),
    ValueSpec(ValueSpec<'a>),
}

// https://pkg.go.dev/go/ast#Decl
#[derive(Debug)]
pub enum Decl<'a> {
    FuncDecl(FuncDecl<'a>),
    GenDecl(GenDecl<'a>),
}

// https://pkg.go.dev/go/ast#Scope
#[derive(Debug)]
pub struct Scope<'a> {
    pub outer: Option<Box<Scope<'a>>>,
    pub objects: BTreeMap<&'a str, Object<'a>>,
}

// https://pkg.go.dev/go/ast#GenDecl
#[derive(Debug)]
pub struct GenDecl<'a> {
    pub doc: Option<CommentGroup>,    // associated documentation; or nil
    pub tok_pos: Position<'a>,        // position of Tok
    pub tok: Token,                   // IMPORT, CONST, TYPE, or VAR
    pub lparen: Option<Position<'a>>, // position of '(', if any
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
    pub x: Box<Expr<'a>>,     // left operand
    pub op_pos: Position<'a>, // position of Op
    pub op: Token,            // operator
    pub y: Box<Expr<'a>>,     // right operand
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
    pub elt: Box<Expr<'a>>,     // ellipsis element type (parameter lists only); or nil
}

// https://pkg.go.dev/go/ast#TypeSpec
#[derive(Debug)]
pub struct TypeSpec<'a> {
    pub doc: Option<CommentGroup>,     // associated documentation; or nil
    pub name: Option<Ident<'a>>,       // type name
    pub assign: Option<Position<'a>>,  // position of '=', if any
    pub type_: Expr<'a>, // *Ident, *ParenExpr, *SelectorExpr, *StarExpr, or any of the *XxxTypes
    pub comment: Option<CommentGroup>, // line comments; or nil
}

// https://pkg.go.dev/go/ast#StructType
#[derive(Debug)]
pub struct StructType<'a> {
    pub struct_: Position<'a>,         // position of "struct" keyword
    pub fields: Option<FieldList<'a>>, // list of field declarations
    pub incomplete: bool,              // true if (source) fields are missing in the Fields list
}

// https://pkg.go.dev/go/ast#StarExpr
#[derive(Debug)]
pub struct StarExpr<'a> {
    pub star: Position<'a>, // position of "*"
    pub x: Box<Expr<'a>>,   // operand
}

// https://pkg.go.dev/go/ast#InterfaceType
#[derive(Debug)]
pub struct InterfaceType<'a> {
    pub interface: Position<'a>,        // position of "interface" keyword
    pub methods: Option<FieldList<'a>>, // list of embedded interfaces, methods, or types
    pub incomplete: bool, // true if (source) methods or types are missing in the Methods list
}

// https://pkg.go.dev/go/ast#DeclStmt
#[derive(Debug)]
pub struct DeclStmt<'a> {
    pub decl: GenDecl<'a>, // *GenDecl with CONST, TYPE, or VAR token
}

// https://pkg.go.dev/go/ast#UnaryExpr
#[derive(Debug)]
pub struct UnaryExpr<'a> {
    pub op_pos: Position<'a>, // position of Op
    pub op: Token,            // operator
    pub x: Box<Expr<'a>>,     // operand
}

// https://pkg.go.dev/go/ast#CallExpr
#[derive(Debug)]
pub struct CallExpr<'a> {
    pub fun: Box<Expr<'a>>,             // function expression
    pub lparen: Position<'a>,           // position of "("
    pub args: Option<Vec<Expr<'a>>>,    // function arguments; or nil
    pub ellipsis: Option<Position<'a>>, // position of "..." (token.NoPos if there is no "...")
    pub rparen: Position<'a>,           // position of ")"
}

// https://pkg.go.dev/go/ast#SelectorExpr
#[derive(Debug)]
pub struct SelectorExpr<'a> {
    pub x: Box<Expr<'a>>, // expression
    pub sel: Ident<'a>,   // field selector
}

// https://pkg.go.dev/go/ast#ExprStmt
#[derive(Debug)]
pub struct ExprStmt<'a> {
    pub x: Expr<'a>, // expression
}

// https://pkg.go.dev/go/ast#SelectorExpr
#[derive(Debug)]
pub struct IfStmt<'a> {
    pub if_: Position<'a>,           // position of "if" keyword
    pub init: Box<Option<Stmt<'a>>>, // initialization statement; or nil
    pub cond: Expr<'a>,              // condition
    pub body: BlockStmt<'a>,
    pub else_: Box<Option<Stmt<'a>>>, // else branch; or nil
}

// https://pkg.go.dev/go/ast#IncDecStmt
#[derive(Debug)]
pub struct IncDecStmt<'a> {
    pub x: Expr<'a>,
    pub tok_pos: Position<'a>, // position of Tok
    pub tok: Token,            // INC or DEC
}

// https://pkg.go.dev/go/ast#ParenExpr
#[derive(Debug)]
pub struct ParenExpr<'a> {
    pub lparen: Position<'a>, // position of "("
    pub x: Box<Expr<'a>>,     // parenthesized expression
    pub rparen: Position<'a>, // position of ")"
}

// https://pkg.go.dev/go/ast#GoStmt
#[derive(Debug)]
pub struct GoStmt<'a> {
    pub go: Position<'a>, // position of "go" keyword
    pub call: CallExpr<'a>,
}

// https://pkg.go.dev/go/ast#FuncLit
#[derive(Debug)]
pub struct FuncLit<'a> {
    pub type_: FuncType<'a>, // function type
    pub body: BlockStmt<'a>, // function body
}

// https://pkg.go.dev/go/ast#ChanType
#[derive(Debug)]
pub struct ChanType<'a> {
    pub begin: Position<'a>, // position of "chan" keyword or "<-" (whichever comes first)
    pub arrow: Option<Position<'a>>, // position of "<-" (token.NoPos if there is no "<-")
    pub dir: u8,             // channel direction
    pub value: Box<Expr<'a>>, // value type
}

// https://pkg.go.dev/go/ast#SendStmt
#[derive(Debug)]
pub struct SendStmt<'a> {
    pub chan: Expr<'a>,
    pub arrow: Position<'a>, // position of "<-"
    pub value: Expr<'a>,
}

// https://pkg.go.dev/go/ast#ForStmt
#[derive(Debug)]
pub struct ForStmt<'a> {
    pub for_: Position<'a>,          // position of "for" keyword
    pub init: Option<Box<Stmt<'a>>>, // initialization statement; or nil
    pub cond: Option<Expr<'a>>,      // condition; or nil
    pub post: Option<Box<Stmt<'a>>>, // post iteration statement; or nil
    pub body: BlockStmt<'a>,
}

// https://pkg.go.dev/go/ast#RangeStmt
#[derive(Debug)]
pub struct RangeStmt<'a> {
    pub for_: Position<'a>,            // position of "for" keyword
    pub key: Option<Expr<'a>>,         // Key, Value may be nil
    pub value: Option<Expr<'a>>,       // Key, Value may be nil
    pub tok_pos: Option<Position<'a>>, // position of Tok; invalid if Key == nil
    pub tok: Option<Token>,            // ILLEGAL if Key == nil, ASSIGN, DEFINE
    pub x: Expr<'a>,                   // value to range over
    pub body: BlockStmt<'a>,
}

// https://pkg.go.dev/go/ast#EmptyStmt
#[derive(Debug)]
pub struct EmptyStmt<'a> {
    pub semicolon: Position<'a>, // position of following ";"
    pub implicit: bool,          // if set, ";" was omitted in the source
}

// https://pkg.go.dev/go/ast#IndexExpr
#[derive(Debug)]
pub struct IndexExpr<'a> {
    pub x: Box<Expr<'a>>,     // expression
    pub lbrack: Position<'a>, // position of "["
    pub index: Box<Expr<'a>>, // index expression
    pub rbrack: Position<'a>, // position of "]"
}

// https://pkg.go.dev/go/ast#MapType
#[derive(Debug)]
pub struct MapType<'a> {
    pub map: Position<'a>,
    pub key: Box<Expr<'a>>,
    pub value: Box<Expr<'a>>,
}

// https://pkg.go.dev/go/ast#CompositeLit
#[derive(Debug)]
pub struct CompositeLit<'a> {
    pub type_: Box<Expr<'a>>,        // literal type; or nil
    pub lbrace: Position<'a>,        // position of "{"
    pub elts: Option<Vec<Expr<'a>>>, // list of composite elements; or nil
    pub rbrace: Position<'a>,        // position of "}"
    pub incomplete: bool,            // true if (source) expressions are missing in the Elts list
}

// https://pkg.go.dev/go/ast#ChanDir
#[derive(Debug)]
pub enum ChanDir {
    SEND = 1 << 0,
    RECV = 1 << 1,
}

// https://pkg.go.dev/go/ast#Spec
#[derive(Debug)]
pub enum Spec<'a> {
    ImportSpec(ImportSpec<'a>),
    TypeSpec(TypeSpec<'a>),
    ValueSpec(ValueSpec<'a>),
}

// https://pkg.go.dev/go/ast#Expr
#[derive(Debug)]
pub enum Expr<'a> {
    BasicLit(BasicLit<'a>),
    BinaryExpr(BinaryExpr<'a>),
    CallExpr(CallExpr<'a>),
    ChanType(ChanType<'a>),
    CompositeLit(CompositeLit<'a>),
    Ellipsis(Ellipsis<'a>),
    FuncLit(FuncLit<'a>),
    FuncType(FuncType<'a>),
    Ident(Ident<'a>),
    IndexExpr(IndexExpr<'a>),
    InterfaceType(InterfaceType<'a>),
    MapType(MapType<'a>),
    ParenExpr(ParenExpr<'a>),
    SelectorExpr(SelectorExpr<'a>),
    StarExpr(StarExpr<'a>),
    StructType(StructType<'a>),
    UnaryExpr(UnaryExpr<'a>),
}

// https://pkg.go.dev/go/ast#Stmt
#[derive(Debug)]
pub enum Stmt<'a> {
    AssignStmt(AssignStmt<'a>),
    BlockStmt(BlockStmt<'a>),
    DeclStmt(DeclStmt<'a>),
    EmptyStmt(EmptyStmt<'a>),
    ExprStmt(ExprStmt<'a>),
    ForStmt(ForStmt<'a>),
    GoStmt(GoStmt<'a>),
    IfStmt(IfStmt<'a>),
    IncDecStmt(IncDecStmt<'a>),
    RangeStmt(RangeStmt<'a>),
    ReturnStmt(ReturnStmt<'a>),
    SendStmt(SendStmt<'a>),
}
