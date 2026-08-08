use super::*;

struct CorruptBlockId;

impl MirPass for CorruptBlockId {
    fn name(&self) -> &'static str {
        "corrupt-block-id"
    }

    fn run(&self, file: &mut mir::File) -> Result<(), Diagnostic> {
        file.functions[0].blocks[0].id = BasicBlockId(99);
        Ok(())
    }
}

#[test]
fn pass_manager_rejects_a_pass_that_breaks_mir() {
    let source = "package main\nfunc main() { println(true) }\n";
    let hir = crate::compiler::lower_to_hir("pass.go", source).unwrap();
    let mir = crate::compiler::lower_to_mir(&hir).unwrap();
    let corrupt = CorruptBlockId;
    let passes: [&dyn MirPass; 1] = [&corrupt];
    let error = PassManager::new(&passes).run(mir).unwrap_err();

    assert_eq!(error.len(), 1);
    assert!(error[0].message.contains("after pass `corrupt-block-id`"));
    assert!(error[0].message.contains("block IDs are not dense"));
}

#[test]
fn normalization_rederives_unsigned_shift_effects_from_the_typed_count() {
    let source =
        "package main\nfunc shift(value uint8, count uint64) uint8 { return value << count }\n";
    let hir = crate::compiler::lower_to_hir("shift.go", source).unwrap();
    let raw = mir::lower_file(&hir).unwrap();
    let normalized = normalize(VerifiedMir::verify(raw).unwrap()).unwrap();
    let shift = normalized
        .as_file()
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.statements)
        .map(|statement| &statement.value)
        .find(|rvalue| {
            matches!(
                rvalue.kind,
                RvalueKind::Binary {
                    op: hir::BinaryOp::Shl,
                    ..
                }
            )
        })
        .expect("unsigned shift rvalue");

    assert!(!shift.effects.may_panic);
    assert_eq!(shift.panic, PanicEdge::None);
}
