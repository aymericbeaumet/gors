use super::*;

#[test]
fn suppressed_array_operand_content_does_not_change_stage_fingerprints() {
    let first = r#"
        package main
        const width = len([4]int{1})
        func main() { println(width) }
    "#;
    let second = r#"
        package main
        const width = len([4]int{2})
        func main() { println(width) }
    "#;
    let changed = r#"
        package main
        const width = len([5]int{2})
        func main() { println(width) }
    "#;
    let (first_hir, first_mir, first_rust) = lower_stages(first);
    let (second_hir, second_mir, second_rust) = lower_stages(second);
    let (changed_hir, changed_mir, changed_rust) = lower_stages(changed);

    assert_eq!(hir_file(&first_hir), hir_file(&second_hir));
    assert_eq!(mir_file(&first_mir), mir_file(&second_mir));
    assert_eq!(rust_ir_file(&first_rust), rust_ir_file(&second_rust));
    assert_ne!(hir_file(&first_hir), hir_file(&changed_hir));
    assert_ne!(mir_file(&first_mir), mir_file(&changed_mir));
    assert_ne!(rust_ir_file(&first_rust), rust_ir_file(&changed_rust));
}

#[test]
fn runtime_array_len_is_fingerprinted_as_executable_hir() {
    let constant = r#"
        package main
        func main() {
            const width = len([4]int{1})
            println(width)
        }
    "#;
    let runtime = r#"
        package main
        func values() [4]int { return [4]int{1, 2, 3, 4} }
        func main() { println(len(values())) }
    "#;
    let (constant_hir, _, _) = lower_stages(constant);
    let (runtime_hir, _, _) = lower_stages(runtime);

    assert_ne!(hir_file(&constant_hir), hir_file(&runtime_hir));
    assert!(format!("{runtime_hir:#?}").contains("ArrayLen"));
    assert!(!format!("{constant_hir:#?}").contains("ArrayLen"));
}
