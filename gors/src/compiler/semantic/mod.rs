//! Name resolution and type checking for the authoritative compiler.

mod expression_lower;
mod expressions;
mod function;
mod positions;
mod statements;

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests;

use std::collections::BTreeMap;

use num_bigint::BigInt;

use crate::ast;
use crate::token::{Position, Token};

use expressions::*;
use function::FunctionLowerer;
use positions::expr_position;

use super::Diagnostic;
use super::hir;
use super::ids::{DefId, FileId, NodeId, SourceSpan};
use super::types::{ConstValue, IntTy, Signature, Ty, UntypedTy};

#[derive(Clone)]
struct FunctionSymbol {
    id: DefId,
    signature: Signature,
}

#[derive(Clone)]
struct ConstantSymbol {
    id: DefId,
    ty: Ty,
    value: ConstValue,
}

struct FileLowerer {
    file_id: FileId,
    file_name: String,
    next_node: u32,
    next_def: u32,
    functions: BTreeMap<String, FunctionSymbol>,
    constants: BTreeMap<String, ConstantSymbol>,
    diagnostics: Vec<Diagnostic>,
}

pub(super) fn lower_file(file: &ast::File<'_>) -> Result<hir::File, Vec<Diagnostic>> {
    let mut lowerer = FileLowerer {
        file_id: FileId(0),
        file_name: file.file_start.file.to_string(),
        next_node: 0,
        next_def: 0,
        functions: BTreeMap::new(),
        constants: BTreeMap::new(),
        diagnostics: Vec::new(),
    };

    lowerer.collect_function_headers(file);
    let constants = lowerer.collect_constants(file);
    if !lowerer.diagnostics.is_empty() {
        return Err(lowerer.diagnostics);
    }

    let mut functions = Vec::new();
    for decl in &file.decls {
        if let ast::Decl::FuncDecl(function) = decl {
            match lowerer.lower_function(function) {
                Ok(function) => functions.push(function),
                Err(diagnostic) => lowerer.diagnostics.push(diagnostic),
            }
        }
    }

    if lowerer.diagnostics.is_empty() {
        Ok(hir::File {
            package: file.name.name.to_string(),
            constants,
            functions,
        })
    } else {
        Err(lowerer.diagnostics)
    }
}

impl FileLowerer {
    fn alloc_node(&mut self) -> NodeId {
        let id = NodeId {
            file: self.file_id,
            ordinal: self.next_node,
        };
        self.next_node += 1;
        id
    }

    fn alloc_def(&mut self) -> DefId {
        let id = DefId(self.next_def);
        self.next_def += 1;
        id
    }

    fn span(&self, position: &Position<'_>) -> SourceSpan {
        SourceSpan {
            file: if position.file.is_empty() {
                self.file_name.clone()
            } else {
                position.file.to_string()
            },
            start: position.offset,
            end: position.offset,
            line: position.line,
            column: position.column,
        }
    }

    fn collect_function_headers(&mut self, file: &ast::File<'_>) {
        for decl in &file.decls {
            let ast::Decl::FuncDecl(function) = decl else {
                continue;
            };
            let span = self.span(&function.name.name_pos);
            if function.recv.is_some() {
                self.diagnostics.push(Diagnostic::unsupported(
                    "methods are not implemented by the HIR/MIR backend",
                    span,
                ));
                continue;
            }
            if function.type_.type_params.is_some() {
                self.diagnostics.push(Diagnostic::unsupported(
                    "generic functions are not implemented by the HIR/MIR backend",
                    span,
                ));
                continue;
            }
            let params = match self.field_types(&function.type_.params) {
                Ok(params) => params,
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    continue;
                }
            };
            let results = match function
                .type_
                .results
                .as_ref()
                .map(|fields| self.field_types(fields))
                .transpose()
            {
                Ok(results) => results.unwrap_or_default(),
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    continue;
                }
            };
            if results.len() > 1 {
                self.diagnostics.push(Diagnostic::unsupported(
                    "multiple-result functions require explicit expression-arity HIR and are not implemented",
                    span,
                ));
                continue;
            }
            if function.name.name == "init" {
                self.diagnostics.push(Diagnostic::unsupported(
                    "package init functions are not implemented by the HIR/MIR backend",
                    span,
                ));
                continue;
            }
            if function.name.name == "main" && (!params.is_empty() || !results.is_empty()) {
                self.diagnostics.push(Diagnostic::semantic(
                    "func main must have no parameters and no results",
                    span,
                ));
                continue;
            }
            let signature = Signature { params, results };
            let symbol = FunctionSymbol {
                id: self.alloc_def(),
                signature,
            };
            if self
                .functions
                .insert(function.name.name.to_string(), symbol)
                .is_some()
            {
                self.diagnostics.push(Diagnostic::semantic(
                    format!("duplicate function {}", function.name.name),
                    self.span(&function.name.name_pos),
                ));
            }
        }
    }

    fn collect_constants(&mut self, file: &ast::File<'_>) -> Vec<hir::Constant> {
        let mut result = Vec::new();
        for decl in &file.decls {
            let ast::Decl::GenDecl(decl) = decl else {
                continue;
            };
            match decl.tok {
                Token::IMPORT => {
                    self.diagnostics.push(Diagnostic::unsupported(
                        "imports are not implemented by the HIR/MIR backend",
                        self.span(&decl.tok_pos),
                    ));
                }
                Token::TYPE | Token::VAR => {
                    self.diagnostics.push(Diagnostic::unsupported(
                        "top-level type and variable declarations are not implemented by the HIR/MIR backend",
                        self.span(&decl.tok_pos),
                    ));
                }
                Token::CONST => {
                    for spec in &decl.specs {
                        let ast::Spec::ValueSpec(spec) = spec else {
                            self.diagnostics.push(Diagnostic::semantic(
                                "const declaration contains a non-value specification",
                                self.span(&decl.tok_pos),
                            ));
                            continue;
                        };
                        let Some(values) = spec.values.as_ref() else {
                            self.diagnostics.push(Diagnostic::unsupported(
                                "implicit repeated const expressions and iota are not implemented",
                                self.span(&decl.tok_pos),
                            ));
                            continue;
                        };
                        if values.len() != spec.names.len() {
                            self.diagnostics.push(Diagnostic::unsupported(
                                "multi-valued const expressions are not implemented",
                                self.span(&decl.tok_pos),
                            ));
                            continue;
                        }
                        let explicit_ty = match spec
                            .type_
                            .as_ref()
                            .map(|ty| self.lower_type(ty))
                            .transpose()
                        {
                            Ok(ty) => ty,
                            Err(diagnostic) => {
                                self.diagnostics.push(diagnostic);
                                continue;
                            }
                        };
                        // A ConstSpec's names enter scope only after every RHS
                        // has been evaluated. Earlier ConstSpecs in the group
                        // remain visible through `self.constants`.
                        let mut pending = Vec::with_capacity(spec.names.len());
                        for (name, value) in spec.names.iter().zip(values) {
                            let span = self.span(&name.name_pos);
                            let (raw_ty, value) = match self.eval_constant(value) {
                                Ok(value) => value,
                                Err(diagnostic) => {
                                    self.diagnostics.push(diagnostic);
                                    continue;
                                }
                            };
                            let ty = explicit_ty
                                .clone()
                                .unwrap_or_else(|| raw_ty.default_typed());
                            if let Err(diagnostic) = ensure_bootstrap_value_type(&ty, &span) {
                                self.diagnostics.push(diagnostic);
                                continue;
                            }
                            if !is_assignable(&raw_ty, &ty) {
                                self.diagnostics.push(Diagnostic::semantic(
                                    format!("constant {} is not assignable to {ty:?}", name.name),
                                    span,
                                ));
                                continue;
                            }
                            if !value.is_representable_as(&ty) {
                                self.diagnostics.push(Diagnostic::semantic(
                                    format!(
                                        "constant {} is not representable as {ty:?}",
                                        name.name
                                    ),
                                    span,
                                ));
                                continue;
                            }
                            pending.push((name, span, ty, value));
                        }
                        for (name, span, ty, value) in pending {
                            let id = self.alloc_def();
                            let symbol = ConstantSymbol {
                                id,
                                ty: ty.clone(),
                                value: value.clone(),
                            };
                            if self
                                .constants
                                .insert(name.name.to_string(), symbol)
                                .is_some()
                                || self.functions.contains_key(name.name)
                            {
                                self.diagnostics.push(Diagnostic::semantic(
                                    format!("duplicate top-level declaration {}", name.name),
                                    span,
                                ));
                                continue;
                            }
                            result.push(hir::Constant {
                                id,
                                name: name.name.to_string(),
                                ty,
                                value,
                                span,
                            });
                        }
                    }
                }
                _ => self.diagnostics.push(Diagnostic::semantic(
                    "invalid top-level declaration token",
                    self.span(&decl.tok_pos),
                )),
            }
        }
        result
    }

    fn field_types(&self, fields: &ast::FieldList<'_>) -> Result<Vec<Ty>, Diagnostic> {
        let mut result = Vec::new();
        for field in &fields.list {
            let Some(type_expr) = field.type_.as_ref() else {
                return Err(Diagnostic::semantic(
                    "field has no type",
                    SourceSpan::synthetic(),
                ));
            };
            let ty = self.lower_type(type_expr)?;
            let count = field.names.as_ref().map_or(1, Vec::len);
            result.extend(std::iter::repeat_n(ty, count));
        }
        Ok(result)
    }

    fn lower_type(&self, expr: &ast::Expr<'_>) -> Result<Ty, Diagnostic> {
        let ast::Expr::Ident(ident) = expr else {
            return Err(Diagnostic::unsupported(
                "only primitive types are implemented by the HIR/MIR backend",
                self.span(&expr_position(expr)),
            ));
        };
        let ty = match ident.name {
            "bool" => Ty::Bool,
            "string" => Ty::String,
            "int" => Ty::Int(IntTy::Int),
            "int8" | "int16" | "int32" | "rune" | "int64" | "uint" | "uint8" | "byte"
            | "uint16" | "uint32" | "uint64" | "uintptr" | "float32" | "float64" => {
                return Err(Diagnostic::unsupported(
                    format!(
                        "type {} is outside the bootstrap bool/int/string runtime frontier",
                        ident.name
                    ),
                    self.span(&ident.name_pos),
                ));
            }
            other => {
                return Err(Diagnostic::unsupported(
                    format!("type {other} is not implemented by the HIR/MIR backend"),
                    self.span(&ident.name_pos),
                ));
            }
        };
        Ok(ty)
    }

    fn eval_constant(&self, expr: &ast::Expr<'_>) -> Result<(Ty, ConstValue), Diagnostic> {
        match expr {
            ast::Expr::BasicLit(literal) => match literal.kind {
                Token::INT => parse_go_integer(literal.value)
                    .map(|value| (Ty::Untyped(UntypedTy::Int), ConstValue::Int(value)))
                    .ok_or_else(|| {
                        Diagnostic::semantic(
                            format!("invalid integer literal {}", literal.value),
                            self.span(&literal.value_pos),
                        )
                    }),
                Token::FLOAT => Ok((
                    Ty::Untyped(UntypedTy::Float),
                    ConstValue::Float(literal.value.replace('_', "")),
                )),
                Token::STRING => parse_go_string(literal.value)
                    .map(|value| (Ty::Untyped(UntypedTy::String), ConstValue::String(value)))
                    .ok_or_else(|| {
                        Diagnostic::semantic(
                            "invalid string literal",
                            self.span(&literal.value_pos),
                        )
                    }),
                Token::CHAR => parse_go_rune(literal.value)
                    .map(|value| {
                        (
                            Ty::Untyped(UntypedTy::Int),
                            ConstValue::Int(value.to_string()),
                        )
                    })
                    .ok_or_else(|| {
                        Diagnostic::semantic("invalid rune literal", self.span(&literal.value_pos))
                    }),
                _ => Err(Diagnostic::unsupported(
                    format!("literal kind {:?} is not implemented", literal.kind),
                    self.span(&literal.value_pos),
                )),
            },
            ast::Expr::Ident(ident) => {
                if let Some(constant) = self.constants.get(ident.name) {
                    Ok((constant.ty.clone(), constant.value.clone()))
                } else if ident.name == "true" || ident.name == "false" {
                    Ok((
                        Ty::Untyped(UntypedTy::Bool),
                        ConstValue::Bool(ident.name == "true"),
                    ))
                } else {
                    Err(Diagnostic::semantic(
                        format!("{} is not a constant", ident.name),
                        self.span(&ident.name_pos),
                    ))
                }
            }
            ast::Expr::BinaryExpr(binary) => {
                let (left_ty, left) = self.eval_constant(&binary.x)?;
                let (right_ty, right) = self.eval_constant(&binary.y)?;
                let op = lower_binary_op(binary.op).ok_or_else(|| {
                    Diagnostic::unsupported(
                        format!("constant operator {:?} is not implemented", binary.op),
                        self.span(&binary.op_pos),
                    )
                })?;
                let operand_ty =
                    exact_common_operand_type(&left_ty, &right_ty).ok_or_else(|| {
                        Diagnostic::semantic(
                            format!("incompatible constant operands {left_ty:?} and {right_ty:?}"),
                            self.span(&binary.op_pos),
                        )
                    })?;
                let runtime_ty = operand_ty.default_typed();
                ensure_bootstrap_value_type(&runtime_ty, &self.span(&binary.op_pos))?;
                validate_binary_operator(op, &runtime_ty, &self.span(&binary.op_pos))?;
                let value = fold_constant_binary(op, &left, &right, &self.span(&binary.op_pos))?
                    .ok_or_else(|| {
                        Diagnostic::unsupported(
                            "constant operation is not implemented by the bootstrap evaluator",
                            self.span(&binary.op_pos),
                        )
                    })?;
                let result_ty = if matches!(
                    op,
                    hir::BinaryOp::Equal
                        | hir::BinaryOp::NotEqual
                        | hir::BinaryOp::Less
                        | hir::BinaryOp::LessEqual
                        | hir::BinaryOp::Greater
                        | hir::BinaryOp::GreaterEqual
                        | hir::BinaryOp::LogicalAnd
                        | hir::BinaryOp::LogicalOr
                ) {
                    Ty::Untyped(UntypedTy::Bool)
                } else {
                    operand_ty
                };
                Ok((result_ty, value))
            }
            ast::Expr::ParenExpr(paren) => self.eval_constant(&paren.x),
            ast::Expr::UnaryExpr(unary) => {
                let (ty, value) = self.eval_constant(&unary.x)?;
                match (unary.op, value) {
                    (Token::ADD, value) => Ok((ty, value)),
                    (Token::SUB, ConstValue::Int(value)) => {
                        let value = BigInt::parse_bytes(value.as_bytes(), 10)
                            .map(|value| (-value).to_string())
                            .ok_or_else(|| {
                                Diagnostic::semantic(
                                    "invalid exact integer",
                                    self.span(&unary.op_pos),
                                )
                            })?;
                        Ok((ty, ConstValue::Int(value)))
                    }
                    (Token::SUB, ConstValue::Float(value)) => {
                        let value = value
                            .strip_prefix('-')
                            .map_or_else(|| format!("-{value}"), str::to_string);
                        Ok((ty, ConstValue::Float(value)))
                    }
                    (Token::NOT, ConstValue::Bool(value)) => Ok((ty, ConstValue::Bool(!value))),
                    _ => Err(Diagnostic::unsupported(
                        "constant unary operation is not implemented",
                        self.span(&unary.op_pos),
                    )),
                }
            }
            _ => Err(Diagnostic::unsupported(
                "constant expression is not implemented by the HIR/MIR backend",
                self.span(&expr_position(expr)),
            )),
        }
    }

    fn lower_function(
        &mut self,
        function: &ast::FuncDecl<'_>,
    ) -> Result<hir::Function, Diagnostic> {
        let symbol = self
            .functions
            .get(function.name.name)
            .cloned()
            .ok_or_else(|| {
                Diagnostic::semantic(
                    format!("missing collected signature for {}", function.name.name),
                    self.span(&function.name.name_pos),
                )
            })?;
        let Some(body) = function.body.as_ref() else {
            return Err(Diagnostic::unsupported(
                "bodyless declarations require an explicit runtime intrinsic",
                self.span(&function.name.name_pos),
            ));
        };

        let node = self.alloc_node();
        let span = self.span(&function.name.name_pos);
        let functions = self.functions.clone();
        let constants = self.constants.clone();
        let mut lowerer = FunctionLowerer {
            file: self,
            functions,
            constants,
            signature: symbol.signature.clone(),
            locals: Vec::new(),
            scopes: vec![BTreeMap::new()],
            named_results: Vec::new(),
            loop_depth: 0,
        };
        let params = lowerer.declare_field_bindings(
            &function.type_.params,
            &symbol.signature.params,
            hir::LocalKind::Parameter,
        )?;
        if let Some(results) = function.type_.results.as_ref() {
            lowerer.named_results =
                lowerer.declare_result_bindings(results, &symbol.signature.results)?;
        } else {
            lowerer.named_results = vec![];
        }
        let body = lowerer.lower_block(body, false)?;
        let named_results = lowerer.named_results.clone();
        let locals = std::mem::take(&mut lowerer.locals);

        Ok(hir::Function {
            id: symbol.id,
            node,
            name: function.name.name.to_string(),
            signature: symbol.signature,
            params,
            named_results,
            locals,
            body,
            span,
        })
    }
}
