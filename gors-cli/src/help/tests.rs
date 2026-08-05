use super::version_line;

#[test]
fn version_line_publishes_the_current_runtime_contract() {
    let expected = format!(
        "runtime-contract={}",
        gors::compiler::db::RuntimeAbiId::current()
    );
    let version = version_line();

    assert!(version.starts_with("gors version gors"));
    assert!(version.ends_with(&expected));
}
