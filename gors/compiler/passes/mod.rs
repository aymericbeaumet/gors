mod hoist_use;
mod inline_fmt;
mod map_type;
mod type_conversion;

pub fn pass(file: &mut syn::File) {
    inline_fmt::pass(file);
    map_type::pass(file);
    type_conversion::pass(file);
    hoist_use::pass(file);
}
