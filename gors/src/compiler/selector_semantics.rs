//! Shared Go selector facts derived from the type environment.

use crate::ast;

use super::typeinfer::{GoType, TypeEnv};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EmbeddedSelectorIndirection {
    Value,
    Pointer,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct EmbeddedSelectorStep {
    pub(super) owner: GoType,
    pub(super) field_name: String,
    pub(super) field_type: GoType,
    pub(super) target: GoType,
    pub(super) indirection: EmbeddedSelectorIndirection,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SelectorField {
    pub(super) owner: GoType,
    pub(super) name: String,
    pub(super) ty: GoType,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SelectorMethod {
    pub(super) receiver: GoType,
    pub(super) key: String,
    pub(super) pointer_receiver: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum SelectorMember {
    Field(SelectorField),
    Method(SelectorMethod),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ResolvedSelector {
    pub(super) embedded: Vec<EmbeddedSelectorStep>,
    pub(super) member: SelectorMember,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum SelectorResolution {
    Found(ResolvedSelector),
    Missing,
    Ambiguous {
        depth: usize,
        candidates: Vec<ResolvedSelector>,
    },
}

#[derive(Clone)]
struct SelectorSearchState {
    owner: GoType,
    embedded: Vec<EmbeddedSelectorStep>,
    ancestor_names: Vec<String>,
    include_pointer_receiver_methods: bool,
}

/// Resolves a field or method selector using Go's shallowest-depth rule.
///
/// `include_pointer_receiver_methods` models an addressable value or a pointer
/// method set. A pointer receiver type enables pointer methods automatically,
/// as does traversing an embedded pointer field.
pub(super) fn resolve_selector(
    receiver: &GoType,
    name: &str,
    include_pointer_receiver_methods: bool,
    env: &TypeEnv,
) -> SelectorResolution {
    let Some((owner, receiver_is_pointer)) = selector_root_owner(receiver, env) else {
        return SelectorResolution::Missing;
    };
    let Some(owner_name) = named_type_name(&owner).map(str::to_string) else {
        return SelectorResolution::Missing;
    };
    let mut frontier = vec![SelectorSearchState {
        owner,
        embedded: Vec::new(),
        ancestor_names: vec![owner_name],
        include_pointer_receiver_methods: include_pointer_receiver_methods || receiver_is_pointer,
    }];

    loop {
        let depth = frontier.first().map_or(0, |state| state.embedded.len());
        let candidates = frontier
            .iter()
            .flat_map(|state| direct_selector_candidates(state, name, env))
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [candidate] => return SelectorResolution::Found(candidate.clone()),
            [_, _, ..] => {
                return SelectorResolution::Ambiguous { depth, candidates };
            }
            [] => {}
        }

        let mut next = Vec::new();
        for state in frontier {
            for (field_name, field_type) in struct_fields_for_owner(&state.owner, env) {
                let Some(owner_name) = named_type_name(&state.owner) else {
                    continue;
                };
                if !env.is_struct_embedded_field(owner_name, &field_name) {
                    continue;
                }
                let Some((target, indirection)) = embedded_selector_target(&field_type, env) else {
                    continue;
                };
                let Some(target_name) = named_type_name(&target) else {
                    continue;
                };
                if state
                    .ancestor_names
                    .iter()
                    .any(|ancestor| ancestor == target_name)
                {
                    continue;
                }
                let mut embedded = state.embedded.clone();
                embedded.push(EmbeddedSelectorStep {
                    owner: state.owner.clone(),
                    field_name,
                    field_type,
                    target: target.clone(),
                    indirection,
                });
                let mut ancestor_names = state.ancestor_names.clone();
                ancestor_names.push(target_name.to_string());
                next.push(SelectorSearchState {
                    owner: target,
                    embedded,
                    ancestor_names,
                    include_pointer_receiver_methods: state.include_pointer_receiver_methods
                        || indirection == EmbeddedSelectorIndirection::Pointer,
                });
            }
        }
        if next.is_empty() {
            return SelectorResolution::Missing;
        }
        frontier = next;
    }
}

fn direct_selector_candidates(
    state: &SelectorSearchState,
    name: &str,
    env: &TypeEnv,
) -> Vec<ResolvedSelector> {
    let mut candidates = struct_fields_for_owner(&state.owner, env)
        .into_iter()
        .filter(|(field_name, _)| field_name == name)
        .map(|(field_name, ty)| ResolvedSelector {
            embedded: state.embedded.clone(),
            member: SelectorMember::Field(SelectorField {
                owner: state.owner.clone(),
                name: field_name,
                ty,
            }),
        })
        .collect::<Vec<_>>();

    if let Some(method) = direct_selector_method(state, name, env) {
        candidates.push(ResolvedSelector {
            embedded: state.embedded.clone(),
            member: SelectorMember::Method(method),
        });
    }
    candidates
}

fn direct_selector_method(
    state: &SelectorSearchState,
    name: &str,
    env: &TypeEnv,
) -> Option<SelectorMethod> {
    let owner_name = named_type_name(&state.owner)?;
    let key = if env.is_interface(owner_name) {
        env.get_interface_methods(owner_name)?
            .iter()
            .any(|method| method == name)
            .then(|| {
                env.get_method_func_key(owner_name, name)
                    .unwrap_or_else(|| format!("{owner_name}.{name}"))
            })?
    } else {
        let key = format!("{owner_name}.{name}");
        if !env.has_func(&key)
            || (!state.include_pointer_receiver_methods && env.method_has_pointer_receiver(&key))
        {
            return None;
        }
        key
    };
    Some(SelectorMethod {
        receiver: state.owner.clone(),
        pointer_receiver: env.method_has_pointer_receiver(&key),
        key,
    })
}

fn selector_root_owner(receiver: &GoType, env: &TypeEnv) -> Option<(GoType, bool)> {
    selector_owner_target(receiver, env)
}

fn embedded_selector_target(
    field_type: &GoType,
    env: &TypeEnv,
) -> Option<(GoType, EmbeddedSelectorIndirection)> {
    selector_owner_target(field_type, env).map(|(target, is_pointer)| {
        let indirection = if is_pointer {
            EmbeddedSelectorIndirection::Pointer
        } else {
            EmbeddedSelectorIndirection::Value
        };
        (target, indirection)
    })
}

fn selector_owner_target(ty: &GoType, env: &TypeEnv) -> Option<(GoType, bool)> {
    match resolve_true_aliases(ty.clone(), env) {
        GoType::Pointer(inner) => canonical_named_type(*inner, env).map(|owner| (owner, true)),
        other => canonical_named_type(other, env).map(|owner| (owner, false)),
    }
}

fn canonical_named_type(ty: GoType, env: &TypeEnv) -> Option<GoType> {
    match resolve_true_aliases(ty, env) {
        GoType::Named(name) | GoType::Interface(name) => Some(GoType::Named(name)),
        GoType::Instantiated { name, args } => Some(GoType::Instantiated { name, args }),
        _ => None,
    }
}

fn resolve_true_aliases(mut ty: GoType, env: &TypeEnv) -> GoType {
    let mut seen = Vec::new();
    loop {
        let is_alias = match &ty {
            GoType::Named(name) | GoType::Instantiated { name, .. } => env.is_type_alias(name),
            _ => false,
        };
        if !is_alias || seen.contains(&ty) {
            return ty;
        }
        seen.push(ty.clone());
        let resolved = env.resolve_alias_outer(&ty);
        if resolved == ty {
            return ty;
        }
        ty = resolved;
    }
}

fn named_type_name(ty: &GoType) -> Option<&str> {
    match ty {
        GoType::Named(name) | GoType::Interface(name) | GoType::Instantiated { name, .. } => {
            Some(name)
        }
        _ => None,
    }
}

fn struct_fields_for_owner(owner: &GoType, env: &TypeEnv) -> Vec<(String, GoType)> {
    match owner {
        GoType::Named(name) | GoType::Interface(name) => env.get_struct_fields(name),
        GoType::Instantiated { name, args } => env.get_struct_fields_with_type_args(name, args),
        _ => Vec::new(),
    }
}

pub(super) fn declared_value_type(name: &str, env: &TypeEnv) -> Option<GoType> {
    env.get_var(name).or_else(|| env.get_top_level_var(name))
}

pub(super) fn selector_base_declared_value_type(
    selector: &ast::SelectorExpr<'_>,
    env: &TypeEnv,
) -> Option<GoType> {
    match unparen_expr(&selector.x) {
        ast::Expr::Ident(base) => declared_value_type(base.name, env),
        ast::Expr::SelectorExpr(base) => {
            let key = qualified_member_key(base)?;
            declared_value_type(&key, env)
        }
        _ => None,
    }
}

pub(super) fn selector_base_is_declared_value(
    selector: &ast::SelectorExpr<'_>,
    env: &TypeEnv,
) -> bool {
    selector_base_declared_value_type(selector, env).is_some()
}

pub(super) fn qualified_member_key(selector: &ast::SelectorExpr<'_>) -> Option<String> {
    let ast::Expr::Ident(base) = selector.x.as_ref() else {
        return None;
    };
    Some(format!("{}.{}", base.name, selector.sel.name))
}

fn unparen_expr<'a>(expr: &'a ast::Expr<'a>) -> &'a ast::Expr<'a> {
    match expr {
        ast::Expr::ParenExpr(paren) => unparen_expr(&paren.x),
        _ => expr,
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
mod tests {
    use super::*;
    use crate::compiler::typeinfer::TypeKind;
    use crate::parser::parse_file;

    fn named(name: &str) -> GoType {
        GoType::Named(name.to_string())
    }

    fn embedded_fields(env: &mut TypeEnv, owner: &str, fields: Vec<(&str, GoType)>) {
        env.set_type_kind(owner, TypeKind::Struct);
        env.set_struct_fields(
            owner,
            fields
                .iter()
                .map(|(name, ty)| ((*name).to_string(), ty.clone()))
                .collect(),
        );
        env.set_struct_embedded_fields(
            owner,
            fields
                .into_iter()
                .map(|(name, _)| name.to_string())
                .collect(),
        );
    }

    fn first_selector_expr<'a>(file: &'a ast::File<'a>) -> &'a ast::SelectorExpr<'a> {
        let ast::Decl::FuncDecl(func) = file.decls.first().expect("function declaration") else {
            panic!("expected function");
        };
        let ast::Stmt::ExprStmt(stmt) = func
            .body
            .as_ref()
            .expect("function body")
            .list
            .first()
            .expect("expression statement")
        else {
            panic!("expected expression statement");
        };
        let ast::Expr::SelectorExpr(selector) = &stmt.x else {
            panic!("expected selector expression");
        };
        selector
    }

    #[test]
    fn selector_base_declared_value_detects_identifier_values() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    T.M
                }
            "#,
        )
        .unwrap();
        let selector = first_selector_expr(&file);
        let mut env = TypeEnv::new();

        assert!(!selector_base_is_declared_value(selector, &env));

        env.set_var("T", GoType::Int);

        assert_eq!(
            selector_base_declared_value_type(selector, &env),
            Some(GoType::Int)
        );
    }

    #[test]
    fn selector_base_declared_value_detects_package_member_values() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    pkg.Value.M
                }
            "#,
        )
        .unwrap();
        let selector = first_selector_expr(&file);
        let mut env = TypeEnv::new();

        assert!(!selector_base_is_declared_value(selector, &env));

        env.set_top_level_var("pkg.Value", GoType::Named("Value".to_string()));

        assert_eq!(
            selector_base_declared_value_type(selector, &env),
            Some(GoType::Named("Value".to_string()))
        );
    }

    #[test]
    fn selector_resolution_preserves_deep_generic_projection_types() {
        let mut env = TypeEnv::new();
        env.set_type_kind("Leaf", TypeKind::Struct);
        env.set_type_param_names("Leaf", vec!["T".to_string()]);
        env.set_struct_fields("Leaf", vec![("value".to_string(), named("T"))]);

        env.set_type_param_names("Middle", vec!["U".to_string()]);
        embedded_fields(
            &mut env,
            "Middle",
            vec![(
                "Leaf",
                GoType::Instantiated {
                    name: "Leaf".to_string(),
                    args: vec![named("U")],
                },
            )],
        );
        embedded_fields(
            &mut env,
            "Outer",
            vec![(
                "Middle",
                GoType::Instantiated {
                    name: "Middle".to_string(),
                    args: vec![GoType::Int],
                },
            )],
        );

        let SelectorResolution::Found(resolved) =
            resolve_selector(&named("Outer"), "value", false, &env)
        else {
            panic!("expected a unique promoted field");
        };
        assert_eq!(resolved.embedded.len(), 2);
        assert_eq!(
            resolved.embedded[0].target,
            GoType::Instantiated {
                name: "Middle".to_string(),
                args: vec![GoType::Int],
            }
        );
        assert_eq!(
            resolved.embedded[1].target,
            GoType::Instantiated {
                name: "Leaf".to_string(),
                args: vec![GoType::Int],
            }
        );
        assert!(matches!(
            resolved.member,
            SelectorMember::Field(SelectorField {
                ty: GoType::Int,
                ..
            })
        ));
    }

    #[test]
    fn selector_resolution_uses_shallowest_member() {
        let mut env = TypeEnv::new();
        env.set_type_kind("Leaf", TypeKind::Struct);
        env.set_struct_fields("Leaf", vec![("value".to_string(), GoType::Int)]);
        embedded_fields(&mut env, "Middle", vec![("Leaf", named("Leaf"))]);
        embedded_fields(&mut env, "Outer", vec![("Middle", named("Middle"))]);
        env.set_struct_fields(
            "Outer",
            vec![
                ("value".to_string(), GoType::Bool),
                ("Middle".to_string(), named("Middle")),
            ],
        );

        let SelectorResolution::Found(resolved) =
            resolve_selector(&named("Outer"), "value", false, &env)
        else {
            panic!("expected the direct field");
        };
        assert!(resolved.embedded.is_empty());
        assert!(matches!(
            resolved.member,
            SelectorMember::Field(SelectorField {
                ty: GoType::Bool,
                ..
            })
        ));
    }

    #[test]
    fn selector_resolution_reports_same_depth_and_diamond_ambiguity() {
        let mut env = TypeEnv::new();
        env.set_type_kind("Leaf", TypeKind::Struct);
        env.set_struct_fields("Leaf", vec![("value".to_string(), GoType::Int)]);
        embedded_fields(&mut env, "Left", vec![("Leaf", named("Leaf"))]);
        embedded_fields(&mut env, "Right", vec![("Leaf", named("Leaf"))]);
        embedded_fields(
            &mut env,
            "Outer",
            vec![("Left", named("Left")), ("Right", named("Right"))],
        );

        let SelectorResolution::Ambiguous { depth, candidates } =
            resolve_selector(&named("Outer"), "value", false, &env)
        else {
            panic!("expected two equally shallow field paths");
        };
        assert_eq!(depth, 2);
        assert_eq!(candidates.len(), 2);
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.embedded.len() == 2)
        );
    }

    #[test]
    fn selector_resolution_terminates_each_cyclic_path_independently() {
        let mut env = TypeEnv::new();
        embedded_fields(
            &mut env,
            "A",
            vec![("B", GoType::Pointer(Box::new(named("B"))))],
        );
        embedded_fields(
            &mut env,
            "B",
            vec![("A", GoType::Pointer(Box::new(named("A"))))],
        );

        assert_eq!(
            resolve_selector(&named("A"), "missing", false, &env),
            SelectorResolution::Missing
        );
    }

    #[test]
    fn selector_resolution_propagates_pointer_method_sets() {
        let mut env = TypeEnv::new();
        env.set_type_kind("Leaf", TypeKind::Struct);
        env.set_func("Leaf.touch", vec![]);
        env.set_pointer_receiver_method("Leaf.touch");
        embedded_fields(&mut env, "ValueOuter", vec![("Leaf", named("Leaf"))]);
        embedded_fields(
            &mut env,
            "PointerOuter",
            vec![("Leaf", GoType::Pointer(Box::new(named("Leaf"))))],
        );

        assert_eq!(
            resolve_selector(&named("ValueOuter"), "touch", false, &env),
            SelectorResolution::Missing
        );
        assert!(matches!(
            resolve_selector(&named("ValueOuter"), "touch", true, &env),
            SelectorResolution::Found(ResolvedSelector {
                member: SelectorMember::Method(SelectorMethod {
                    pointer_receiver: true,
                    ..
                }),
                ..
            })
        ));
        assert!(matches!(
            resolve_selector(&named("PointerOuter"), "touch", false, &env),
            SelectorResolution::Found(ResolvedSelector {
                member: SelectorMember::Method(SelectorMethod {
                    pointer_receiver: true,
                    ..
                }),
                ..
            })
        ));
    }

    #[test]
    fn selector_resolution_rejects_ambiguous_promoted_methods() {
        let mut env = TypeEnv::new();
        for name in ["Left", "Right"] {
            env.set_type_kind(name, TypeKind::Struct);
            env.set_func(&format!("{name}.read"), vec![GoType::Int]);
        }
        embedded_fields(
            &mut env,
            "Outer",
            vec![("Left", named("Left")), ("Right", named("Right"))],
        );

        assert!(matches!(
            resolve_selector(&named("Outer"), "read", false, &env),
            SelectorResolution::Ambiguous { depth: 1, .. }
        ));

        env.set_func("Outer.read", vec![GoType::Int]);
        assert!(matches!(
            resolve_selector(&named("Outer"), "read", false, &env),
            SelectorResolution::Found(ResolvedSelector {
                embedded,
                member: SelectorMember::Method(SelectorMethod { key, .. }),
            }) if embedded.is_empty() && key == "Outer.read"
        ));
    }
}
