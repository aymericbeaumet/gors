use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    MapType.visit_file_mut(file);
}

struct MapType;

impl VisitMut for MapType {
    fn visit_type_path_mut(&mut self, type_path: &mut syn::TypePath) {
        visit_mut::visit_type_path_mut(self, type_path);
        if type_path.qself.is_some() || type_path.path.leading_colon.is_some() {
            return;
        }
        if type_path.path.segments.len() != 1 {
            return;
        }
        let Some(segment) = type_path.path.segments.first_mut() else {
            return;
        };
        let name = match segment.ident.to_string().as_str() {
            "bool" => "bool",
            "byte" => "u8",
            "rune" => "i32",
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
            "uintptr" => "usize",
            _ => return,
        };
        segment.ident = quote::format_ident!("{}", name);
    }
}
