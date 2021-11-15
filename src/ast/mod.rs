mod printer;

use crate::token::{Position, Token};
use std::collections::HashMap;

pub fn fprint<W: std::io::Write>(w: &mut W, file: &File) -> Result<(), Box<dyn std::error::Error>> {
    let mut p = printer::Printer::new(w);
    p.print(file)
}

// https://pkg.go.dev/go/ast#CommentGroup
pub struct CommentGroup {
    // List []*Comment // len(List) > 0
}

pub enum Decl<'a> {
    FuncDecl(FuncDecl<'a>),
    GenDecl(GenDecl<'a>),
}

// https://pkg.go.dev/go/ast#FieldList
pub struct FieldList {
    //Opening token.Pos // position of opening parenthesis/brace, if any
//List    []*Field  // field list; or nil
//Closing token.Pos // position of closing parenthesis/brace, if any
}

// https://pkg.go.dev/go/ast#File
pub struct File<'a> {
    pub doc: Option<CommentGroup>,   // associated documentation; or nil
    pub package: Position<'a>,       // position of "package" keyword
    pub name: Ident<'a>,             // package name
    pub decls: Vec<Decl<'a>>,        // top-level declarations; or nil
    pub scope: Option<Scope>,        // package scope (this file only)
    pub imports: Vec<ImportSpec>,    // imports in this file
    pub unresolved: Vec<Ident<'a>>,  // unresolved identifiers in this file
    pub comments: Vec<CommentGroup>, // list of all comments in the source file
}

// https://pkg.go.dev/go/ast#FuncDecl
pub struct FuncDecl<'a> {
    //doc  *CommentGroup // associated documentation; or nil
    //recv *FieldList    // receiver (methods); or nil (functions)
    pub name: Ident<'a>, // function/method name
    pub type_: FuncType, // function signature: type and value parameters, results, and position of "func" keyword
                         //body *BlockStmt    // function body; or nil for external (non-Go) function
}

// https://pkg.go.dev/go/ast#FuncType
pub struct FuncType {
    //Func    token.Pos  // position of "func" keyword (token.NoPos if there is no "func")
    pub params: FieldList, // (incoming) parameters; non-nil
                           //results: FieldList, // (outgoing) results; or nil
}

// https://pkg.go.dev/go/ast#GenDecl
pub struct GenDecl<'a> {
    pub doc: Option<CommentGroup>, // associated documentation; or nil
    pub tok_pos: Position<'a>,     // position of Tok
    pub tok: Token,                // IMPORT, CONST, TYPE, or VAR
    pub lparen: Position<'a>,      // position of '(', if any
    //Specs  []Spec
    pub rparen: Position<'a>, // position of ')', if any
}

// https://pkg.go.dev/go/ast#Ident
pub struct Ident<'a> {
    pub name_pos: Position<'a>, // identifier position
    pub name: &'a str,          // identifier name
    pub obj: Option<Object>,    // denoted object; or nil
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
pub struct Object {
    //Kind ObjKind
//Name string      // declared name
//Decl interface{} // corresponding Field, XxxSpec, FuncDecl, LabeledStmt, AssignStmt, Scope; or nil
//Data interface{} // object-specific data; or nil
//Type interface{} // placeholder for type information; may be nil
}

// https://pkg.go.dev/go/ast#Scope
pub struct Scope {
    pub outer: Box<Option<Scope>>,
    pub objects: HashMap<String, Object>,
}
