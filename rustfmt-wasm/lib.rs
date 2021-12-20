use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn format(input: String) -> String {
    console_error_panic_hook::set_once();

    //
    let mut out = vec![];

    let mut config = rustfmt_nightly::Config::default();
    config.set().emit_mode(rustfmt_nightly::EmitMode::Stdout);
    config.set().edition(rustfmt_nightly::Edition::Edition2021);
    config.set().verbose(rustfmt_nightly::Verbosity::Quiet);

    {
        let mut session = rustfmt_nightly::Session::new(config, Some(&mut out));
        session.format(rustfmt_nightly::Input::Text(input)).unwrap();
    }

    let formatted = String::from_utf8(out).unwrap();
    //

    formatted
}
