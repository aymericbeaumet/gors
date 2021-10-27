pub mod ser;

// https://pkg.go.dev/go/ast#File
#[derive(Debug)]
pub struct File {
    pub filename: String,
    pub name: Ident,
    pub decls: Vec<Decl>,
}

// https://pkg.go.dev/go/ast#Ident
#[derive(Debug)]
pub struct Ident {
    pub name: String,
}

// https://pkg.go.dev/go/ast#Decl
#[derive(Debug)]
pub enum Decl {
    FuncDecl(FuncDecl),
}

// https://pkg.go.dev/go/ast#FuncDecl
#[derive(Debug)]
pub struct FuncDecl {
    pub name: Ident,
    pub type_: FuncType,
    pub body: BlockStmt,
}

// https://pkg.go.dev/go/ast#FuncType
#[derive(Debug)]
pub struct FuncType {
    pub params: FieldList,
}

// https://pkg.go.dev/go/ast#FieldList
#[derive(Debug)]
pub struct FieldList {}

// https://pkg.go.dev/go/ast#BlockStmt
#[derive(Debug)]
pub struct BlockStmt {}
