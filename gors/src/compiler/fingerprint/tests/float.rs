use super::*;

#[test]
fn exact_float_width_changes_every_semantic_stage_fingerprint() {
    let float32 = lower_stages(
        "package main\nfunc twice(value float32) float32 { return value + value }\nfunc main() {}\n",
    );
    let float64 = lower_stages(
        "package main\nfunc twice(value float64) float64 { return value + value }\nfunc main() {}\n",
    );

    assert_ne!(
        hir_function(hir_named(&float32.0, "twice")),
        hir_function(hir_named(&float64.0, "twice"))
    );
    assert_ne!(
        mir_function(mir_named(&float32.1, "twice")),
        mir_function(mir_named(&float64.1, "twice"))
    );
    assert_ne!(
        rust_ir_function(rust_ir_named(&float32.2, "twice")),
        rust_ir_function(rust_ir_named(&float64.2, "twice"))
    );
}

#[test]
fn rust_ir_fingerprints_encode_complex_component_width() {
    let complex64 = lower_stages(
        "package main\nfunc part(value complex64) float32 { return real(value) }\nfunc main() {}\n",
    );
    let complex128 = lower_stages(
        "package main\nfunc part(value complex128) float64 { return real(value) }\nfunc main() {}\n",
    );

    assert_ne!(
        rust_ir_function(rust_ir_named(&complex64.2, "part")),
        rust_ir_function(rust_ir_named(&complex128.2, "part"))
    );
}
