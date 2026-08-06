//! Structural interface satisfaction and explicit dynamic-value construction.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::SyntaxSource;
use crate::compiler::types::{InterfaceMethod, Signature, Ty};

use super::FunctionLowerer;
use super::expressions::{default_expr_type, ensure_bootstrap_value_type};

pub(super) fn error_interface_ty() -> Ty {
    Ty::Interface(vec![InterfaceMethod {
        name: "Error".to_owned(),
        signature: Signature {
            params: Vec::new(),
            results: vec![Ty::String],
            variadic: false,
        },
    }])
}

impl FunctionLowerer {
    pub(super) fn coerce_interface_value(
        &mut self,
        mut value: hir::Expr,
        expected: &Ty,
        syntax_source: SyntaxSource,
    ) -> Result<hir::Expr, Diagnostic> {
        let source = value.source;
        let Ty::Interface(expected_methods) = expected.underlying() else {
            return Err(Diagnostic::backend(
                "interface coercion received a non-interface destination",
            ));
        };
        if let Ty::Interface(actual_methods) = value.ty.underlying() {
            if !interface_contains(actual_methods, expected_methods) {
                return Err(Diagnostic::semantic(
                    format!("cannot use {:?} as {expected:?}", value.ty),
                    source,
                ));
            }
            value.ty = expected.clone();
            return Ok(value);
        }
        if matches!(value.ty, Ty::Untyped(_)) {
            value = default_expr_type(value, source)?;
        }
        ensure_bootstrap_value_type(&value.ty, source)?;
        if !self.concrete_implements(&value.ty, expected_methods) {
            return Err(Diagnostic::semantic(
                format!("type {:?} does not implement {expected:?}", value.ty),
                source,
            ));
        }
        let type_identity = dynamic_type_identity(&value.ty).ok_or_else(|| {
            Diagnostic::unsupported(
                format!(
                    "interface values do not yet support dynamic type {:?}",
                    value.ty
                ),
                source,
            )
        })?;
        let effects = value.effects.union(hir::Effects {
            may_call: true,
            may_allocate: value.ty.bootstrap_i64_struct_fields().is_some(),
            ..hir::Effects::default()
        });
        let node = self.alloc_node(syntax_source)?;
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::InterfaceValue {
                value: Box::new(value),
                type_identity,
            },
            ty: expected.clone(),
            category: hir::ValueCategory::Value,
            effects,
            source: SourceRef::node(node),
        })
    }

    fn concrete_implements(&self, concrete: &Ty, required: &[InterfaceMethod]) -> bool {
        if required.is_empty() {
            return true;
        }
        let Some((definition, pointer)) = receiver_definition(concrete) else {
            return false;
        };
        required.iter().all(|required| {
            self.methods
                .get(&(definition, required.name.clone()))
                .is_some_and(|method| {
                    (pointer || !method.pointer_receiver)
                        && method.signature.variadic == required.signature.variadic
                        && method.signature.params.get(1..) == Some(&required.signature.params)
                        && method.signature.results == required.signature.results
                })
        })
    }
}

fn interface_contains(actual: &[InterfaceMethod], required: &[InterfaceMethod]) -> bool {
    required.iter().all(|required| {
        actual
            .iter()
            .any(|actual| actual.name == required.name && actual.signature == required.signature)
    })
}

fn receiver_definition(ty: &Ty) -> Option<(crate::compiler::ids::DefId, bool)> {
    match ty {
        Ty::Named { definition, .. } => Some((*definition, false)),
        Ty::Pointer(element) => match element.as_ref() {
            Ty::Named { definition, .. } => Some((*definition, true)),
            _ => None,
        },
        _ => None,
    }
}

fn dynamic_type_identity(ty: &Ty) -> Option<Vec<u8>> {
    let identity = match ty {
        Ty::Bool => "builtin:bool".to_owned(),
        Ty::Int(crate::compiler::types::IntTy::Int) => "builtin:int".to_owned(),
        Ty::String => "builtin:string".to_owned(),
        Ty::Named { definition, .. } => format!("named:{definition}"),
        Ty::Pointer(element) => match element.as_ref() {
            Ty::Named { definition, .. } => format!("pointer:named:{definition}"),
            _ => return None,
        },
        _ => return None,
    };
    Some(identity.into_bytes())
}
