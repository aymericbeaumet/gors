mod printer;
use crate::token::Position;

pub fn fprint<W: std::io::Write>(w: &mut W, file: &File) -> Result<(), Box<dyn std::error::Error>> {
    let mut p = printer::Printer::new(w);
    p.print(file)
}

// https://pkg.go.dev/go/ast#CommentGroup
pub struct CommentGroup {
    // List []*Comment // len(List) > 0
}

// https://pkg.go.dev/go/ast#File
pub struct File<'a> {
    pub doc: Option<CommentGroup>, // associated documentation; or nil
    pub package: Position<'a>,     // position of "package" keyword
    pub name: Option<Ident<'a>>,   // package name
                                   //Decls      []Decl          // top-level declarations; or nil
                                   //Scope      *Scope          // package scope (this file only)
                                   //Imports    []*ImportSpec   // imports in this file
                                   //Unresolved []*Ident        // unresolved identifiers in this file
                                   //Comments   []*CommentGroup // list of all comments in the source file
}

// https://pkg.go.dev/go/ast#Ident
pub struct Ident<'a> {
    pub name_pos: Position<'a>, // identifier position
    pub name: &'a str,          // identifier name
    pub obj: Option<Object>,    // denoted object; or nil
}

// https://pkg.go.dev/go/ast#Object
pub struct Object {
    //Kind ObjKind
//Name string      // declared name
//Decl interface{} // corresponding Field, XxxSpec, FuncDecl, LabeledStmt, AssignStmt, Scope; or nil
//Data interface{} // object-specific data; or nil
//Type interface{} // placeholder for type information; may be nil
}
