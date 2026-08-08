use super::*;

const SOURCE: &str = r#"
    package main
    type Text string
    type Rune rune
    type Runes []Rune
    func fromRune(value Rune) Text { return Text(value) }
    func fromRunes(values Runes) Text { return Text(values) }
    func toRunes(value Text) Runes { return Runes(value) }
    func main() {
        println(fromRune('a'), fromRunes(Runes{'b'}), len(toRunes("c")))
    }
"#;

#[test]
fn rune_conversion_lowering_selects_the_complete_typed_runtime_abi() {
    let file = lower(SOURCE);
    let requirement = file.verify().expect("rune conversion Rust IR must verify");
    for operation in [
        RuntimeOp::GoStringFromRune,
        RuntimeOp::GoStringFromSliceRunes,
        RuntimeOp::GoStringToSliceRunes,
    ] {
        assert!(requirement.contains(operation), "missing {operation:?}");
    }
}

#[test]
fn rune_conversion_verifier_rejects_reversed_runtime_direction() {
    let mut file = lower(SOURCE);
    *runtime_call_target_mut(&mut file, RuntimeOp::GoStringToSliceRunes) =
        RuntimeOp::GoStringFromSliceRunes;
    refresh_test_effects(&mut file);

    let error = file.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("runtime call argument 0 type mismatch"),
        "{error:?}"
    );
}
