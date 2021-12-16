use syn::visit_mut::{self, VisitMut};

pub struct MapType;

impl VisitMut for MapType {
    fn visit_ident_mut(&mut self, ident: &mut syn::Ident) {
        let name = match ident.to_string().as_str() {
            "bool" => "bool",
            "rune" => "u32",
            "string" => "String",
            "float32" => "f32",
            "float64" => "f64",
            "int" => "isize",
            "int8" => "i8",
            "int16" => "i16",
            "int32" => "i32",
            "int64" => "i64",
            "uint" => "usize",
            "uint8" => "u8",
            "uint16" => "u16",
            "uint32" => "u32",
            "uint64" => "u64",
            _ => return,
        };
        *ident = syn::Ident::new(name, ident.span());

        visit_mut::visit_ident_mut(self, ident);
    }
}
