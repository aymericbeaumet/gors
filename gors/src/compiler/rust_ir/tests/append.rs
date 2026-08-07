use super::*;

#[test]
fn whole_slice_append_calls_publish_exact_runtime_requirements() {
    let file = lower(
        r#"
            package main
            func ints(destination, source []int) []int {
                return append(destination, source...)
            }
            func interfaces(destination, source []any) []any {
                return append(destination, source...)
            }
            func main() {}
        "#,
    );

    assert_eq!(
        runtime_operations_in_execution_order(&file, "ints"),
        [RuntimeOp::GoSliceI64AppendSlice]
    );
    assert_eq!(
        runtime_operations_in_execution_order(&file, "interfaces"),
        [RuntimeOp::GoSliceInterfaceAppend]
    );

    let requirement = file.verify().expect("whole-slice append Rust IR");
    assert_eq!(requirement.operation_ids().collect::<Vec<_>>(), [220, 221]);
}
