pub(super) fn is_type_name(name: &str) -> bool {
    matches!(
        name,
        "any"
            | "bool"
            | "byte"
            | "complex64"
            | "complex128"
            | "error"
            | "float32"
            | "float64"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "rune"
            | "string"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_go_predeclared_type_names() {
        for name in [
            "any",
            "bool",
            "byte",
            "complex64",
            "complex128",
            "error",
            "float32",
            "float64",
            "int",
            "int8",
            "int16",
            "int32",
            "int64",
            "rune",
            "string",
            "uint",
            "uint8",
            "uint16",
            "uint32",
            "uint64",
            "uintptr",
        ] {
            assert!(is_type_name(name), "{name}");
        }
    }

    #[test]
    fn rejects_predeclared_non_type_names() {
        for name in [
            "_", "nil", "true", "false", "iota", "append", "cap", "len", "make", "new", "panic",
        ] {
            assert!(!is_type_name(name), "{name}");
        }
    }
}
