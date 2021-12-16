mod inline_fmt;
mod map_type;
mod type_conversion;

use syn::visit_mut::VisitMut;

pub fn apply(file: &mut syn::File) {
    inline_fmt::InlineFmt.visit_file_mut(file);
    map_type::MapType.visit_file_mut(file);
    type_conversion::TypeConversion.visit_file_mut(file);
}
