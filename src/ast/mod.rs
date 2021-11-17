mod printer;

use crate::token::Position;
use std::collections::HashMap;

pub fn fprint<W: std::io::Write>(
    w: &mut W,
    file: &mut File,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut p = printer::Printer::new(w);
    p.print(file)
}

// https://pkg.go.dev/go/ast#CommentGroup
pub struct CommentGroup {
    // List []*Comment // len(List) > 0
}

pub enum Decl<'a> {
    FuncDecl(&'a mut FuncDecl<'a>),
}

// https://pkg.go.dev/go/ast#FieldList
pub struct FieldList<'a> {
    pub opening: Position<'a>, // position of opening parenthesis/brace, if any
    pub list: Vec<&'a mut Field>, // field list; or nil
    pub closing: Position<'a>, // position of closing parenthesis/brace, if any
}

// https://pkg.go.dev/go/ast#Field
pub struct Field {}

// https://pkg.go.dev/go/ast#File
pub struct File<'a> {
    pub doc: Option<&'a mut CommentGroup>, // associated documentation; or nil
    pub package: Position<'a>,             // position of "package" keyword
    pub name: &'a mut Ident<'a>,           // package name
    pub decls: Vec<&'a mut Decl<'a>>,      // top-level declarations; or nil
    pub scope: Option<&'a mut Scope<'a>>,  // package scope (this file only)
    pub imports: Vec<&'a mut ImportSpec>,  // imports in this file
    pub unresolved: Vec<&'a mut Ident<'a>>, // unresolved identifiers in this file
    pub comments: Vec<&'a mut CommentGroup>, // list of all comments in the source file
}

// https://pkg.go.dev/go/ast#FuncDecl
pub struct FuncDecl<'a> {
    pub doc: Option<&'a mut CommentGroup>, // associated documentation; or nil
    pub recv: Option<&'a mut FieldList<'a>>, // receiver (methods); or nil (functions)
    pub name: &'a mut Ident<'a>,           // function/method name
    pub type_: &'a mut FuncType<'a>, // function signature: type and value parameters, results, and position of "func" keyword
    pub body: Option<&'a mut BlockStmt<'a>>, // function body; or nil for external (non-Go) function
}

// https://pkg.go.dev/go/ast#BlockStmt
pub struct BlockStmt<'a> {
    pub lbrace: Position<'a>, // position of "{"
    pub list: Vec<&'a mut Stmt>,
    pub rbrace: Position<'a>, // position of "}", if any (may be absent due to syntax error)
}

// https://pkg.go.dev/go/ast#Stmt
pub struct Stmt {}

// https://pkg.go.dev/go/ast#FuncType
pub struct FuncType<'a> {
    pub func: Position<'a>, // position of "func" keyword (token.NoPos if there is no "func")
    pub params: &'a mut FieldList<'a>, // (incoming) parameters; non-nil
                            //pub results: FieldList<'a>, // (outgoing) results; or nil
}

// https://pkg.go.dev/go/ast#Ident
pub struct Ident<'a> {
    pub name_pos: Position<'a>,          // identifier position
    pub name: &'a str,                   // identifier name
    pub obj: Option<&'a mut Object<'a>>, // denoted object; or nil
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
    pub kind: &'a mut ObjKind,
    pub name: &'a str,                     // declared name
    pub decl: Option<&'a mut ObjDecl<'a>>, // corresponding Field, XxxSpec, FuncDecl, LabeledStmt, AssignStmt, Scope; or nil
    pub data: Option<()>,                  // object-specific data; or nil
    pub type_: Option<()>,                 // placeholder for type information; may be nil
}

// https://pkg.go.dev/go/ast#ObjKind
#[derive(Debug)]
pub enum ObjKind {
    //Pkg, // package
    //Con, // constant
    //Typ, // type
    //Var, // variable
    Fun, // function or method
         //Lbl, // label
}

pub enum ObjDecl<'a> {
    FuncDecl(&'a mut FuncDecl<'a>),
}

// https://pkg.go.dev/go/ast#Scope
pub struct Scope<'a> {
    pub outer: Option<&'a mut Scope<'a>>,
    pub objects: HashMap<String, &'a mut Object<'a>>,
}
