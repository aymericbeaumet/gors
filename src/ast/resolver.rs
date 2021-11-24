use crate::ast::{self, Visitor};
use std::collections::BTreeMap;

pub struct Resolver<'a> {
    pub idents: Vec<&'a ast::Ident<'a>>,
    pub objects: BTreeMap<&'a str, &'a ast::Object<'a>>,
}

impl Resolver<'_> {
    pub fn new() -> Self {
        Self {
            idents: Vec::new(),
            objects: BTreeMap::new(),
        }
    }
}

impl<'a> Visitor<'a> for Resolver<'a> {
    fn FuncDecl(&mut self, func_decl: &'a ast::FuncDecl<'a>) {
        if let Some(o) = func_decl.name.obj.get() {
            self.objects.insert(func_decl.name.name, o);
        }
    }

    fn Ident(&mut self, ident: &'a ast::Ident<'a>) {
        self.idents.push(ident);
    }

    fn ValueSpec(&mut self, value_spec: &'a ast::ValueSpec<'a>) {
        for name in value_spec.names.iter() {
            if let Some(o) = name.obj.get() {
                self.objects.insert(name.name, o);
            }
        }
    }
}
