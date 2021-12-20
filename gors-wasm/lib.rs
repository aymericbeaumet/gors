use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn build(input: String, rustfmt: &js_sys::Function) -> String {
    console_error_panic_hook::set_once();

    let ast = gors::parser::parse_file("main.go", &input).unwrap();
    let compiled = gors::compiler::compile(ast).unwrap();

    let mut w = vec![];
    gors::codegen::fprint(&mut w, compiled, |code| {
        rustfmt
            .call1(&JsValue::null(), &JsValue::from_str(code))
            .unwrap()
            .as_string()
            .unwrap()
    })
    .unwrap();

    String::from_utf8(w).unwrap()
}
