use serde::{Deserialize, Serialize};

// https://pkg.go.dev/go/ast#File
#[derive(Serialize, Deserialize, Debug)]
pub struct File {
    #[serde(rename = "Name")]
    pub name: Ident,
}

// https://pkg.go.dev/go/ast#Ident
#[derive(Serialize, Deserialize, Debug)]
pub struct Ident {
    #[serde(rename = "Name")]
    pub name: String,
}
