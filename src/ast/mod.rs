mod printer;

use crate::ast;
use crate::token::Position;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

pub fn fprint<W: std::io::Write>(w: &mut W, file: &File) -> Result<(), Box<dyn std::error::Error>> {
    let mut p = printer::Printer::new(w);
    p.print(file)
}

// https://pkg.go.dev/go/ast#CommentGroup
pub struct CommentGroup {
    // List []*Comment // len(List) > 0
}

// https://pkg.go.dev/go/ast#FieldList
pub struct FieldList<'a> {
    pub opening: Position<'a>, // position of opening parenthesis/brace, if any
    pub list: Vec<&'a Field>,  // field list; or nil
    pub closing: Position<'a>, // position of closing parenthesis/brace, if any
}

// https://pkg.go.dev/go/ast#Field
pub struct Field {}

// https://pkg.go.dev/go/ast#File
pub struct File<'a> {
    pub doc: Option<&'a CommentGroup>, // associated documentation; or nil
    pub package: Position<'a>,         // position of "package" keyword
    pub name: &'a Ident<'a>,           // package name
    pub decls: Vec<&'a Decl<'a>>,      // top-level declarations; or nil
    pub scope: Option<&'a Scope<'a>>,  // package scope (this file only)
    pub imports: Vec<&'a ImportSpec>,  // imports in this file
    pub unresolved: Vec<&'a Ident<'a>>, // unresolved identifiers in this file
    pub comments: Vec<&'a CommentGroup>, // list of all comments in the source file
}

// https://pkg.go.dev/go/ast#FuncDecl
pub struct FuncDecl<'a> {
    pub doc: Option<&'a CommentGroup>, // associated documentation; or nil
    pub recv: Option<&'a FieldList<'a>>, // receiver (methods); or nil (functions)
    pub name: &'a Ident<'a>,           // function/method name
    pub type_: &'a FuncType<'a>, // function signature: type and value parameters, results, and position of "func" keyword
    pub body: Option<&'a BlockStmt<'a>>, // function body; or nil for external (non-Go) function
}

// https://pkg.go.dev/go/ast#BlockStmt
pub struct BlockStmt<'a> {
    pub lbrace: Position<'a>, // position of "{"
    pub list: Vec<&'a Stmt>,
    pub rbrace: Position<'a>, // position of "}", if any (may be absent due to syntax error)
}

// https://pkg.go.dev/go/ast#Stmt
pub struct Stmt {}

// https://pkg.go.dev/go/ast#FuncType
pub struct FuncType<'a> {
    pub func: Position<'a>, // position of "func" keyword (token.NoPos if there is no "func")
    pub params: &'a FieldList<'a>, // (incoming) parameters; non-nil
                            //pub results: FieldList<'a>, // (outgoing) results; or nil
}

// https://pkg.go.dev/go/ast#Ident
pub struct Ident<'a> {
    pub name_pos: Position<'a>,                       // identifier position
    pub name: &'a str,                                // identifier name
    pub obj: std::cell::Cell<Option<&'a Object<'a>>>, // denoted object; or nil
}

// https://pkg.go.dev/go/ast#ImportSpec
pub struct ImportSpec {
    //Doc     *CommentGroup // associated documentation; or nil
//Name    *Ident        // local package name (including "."); or nil
//Path    *BasicLit     // import path
//Comment *CommentGroup // line comments; or nil
//EndPos  token.Pos     // end of spec (overrides Path.Pos if nonzero)
}

// https://pkg.go.dev/go/ast#Object
pub struct Object<'a> {
    pub kind: ObjKind,
    pub name: &'a str,             // declared name
    pub decl: Option<ObjDecl<'a>>, // corresponding Field, XxxSpec, FuncDecl, LabeledStmt, AssignStmt, Scope; or nil
    pub data: Option<()>,          // object-specific data; or nil
    pub type_: Option<()>,         // placeholder for type information; may be nil
}

impl<'a> Hash for &Object<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash((*self) as *const ast::Object, state)
    }
}

// https://pkg.go.dev/go/ast#ObjKind
pub enum ObjKind {
    //Pkg, // package
    //Con, // constant
    //Typ, // type
    //Var, // variable
    Fun, // function or method
         //Lbl, // label
}

pub enum ObjDecl<'a> {
    FuncDecl(&'a FuncDecl<'a>),
}

impl<'a> Hash for ObjDecl<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            ast::ObjDecl::FuncDecl(decl) => std::ptr::hash((*decl) as *const ast::FuncDecl, state),
        }
    }
}

pub enum Decl<'a> {
    FuncDecl(&'a FuncDecl<'a>),
}

impl<'a> Hash for Decl<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            ast::Decl::FuncDecl(decl) => std::ptr::hash((*decl) as *const ast::FuncDecl, state),
        }
    }
}

// https://pkg.go.dev/go/ast#Scope
pub struct Scope<'a> {
    pub outer: Option<&'a Scope<'a>>,
    pub objects: HashMap<&'a str, &'a Object<'a>>,
}
