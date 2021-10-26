use serde::{Deserialize, Serialize};

// https://pkg.go.dev/go/ast#File
#[derive(Serialize, Deserialize, Debug)]
pub struct File {
    #[serde(rename = "Name")]
    pub name: Ident,
    #[serde(rename = "Decls")]
    pub decls: Vec<Decl>,
}

// https://pkg.go.dev/go/ast#Ident
#[derive(Serialize, Deserialize, Debug)]
pub struct Ident {
    #[serde(rename = "Name")]
    pub name: String,
}

// https://pkg.go.dev/go/ast#Decl
#[derive(Serialize, Deserialize, Debug)]
pub enum Decl {
    FuncDecl(FuncDecl),
}

// https://pkg.go.dev/go/ast#FuncDecl
#[derive(Serialize, Deserialize, Debug)]
pub struct FuncDecl {
    #[serde(rename = "Name")]
    pub name: Ident,
    #[serde(rename = "FuncType")]
    pub type_: FuncType,
    #[serde(rename = "Body")]
    pub body: BlockStmt,
}

// https://pkg.go.dev/go/ast#FuncType
#[derive(Serialize, Deserialize, Debug)]
pub struct FuncType {
    #[serde(rename = "Params")]
    pub params: FieldList,
}

// https://pkg.go.dev/go/ast#FieldList
#[derive(Serialize, Deserialize, Debug)]
pub struct FieldList {}

// https://pkg.go.dev/go/ast#BlockStmt
#[derive(Serialize, Deserialize, Debug)]
pub struct BlockStmt {}
