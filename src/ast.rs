// https://pkg.go.dev/go/ast#File
#[derive(Debug)]
pub struct File {
    pub Name: Ident,
}

// https://pkg.go.dev/go/ast#Ident
#[derive(Debug)]
pub struct Ident {
    pub Name: String,
}
