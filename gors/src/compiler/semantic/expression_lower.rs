//! Typed expression and direct-call lowering over owned structural syntax.

use crate::token::Token;

use super::FunctionLowerer;
use super::eval_constant;
use super::expressions::*;
use super::lower_type;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{LocalId, NodeId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{ComplexTy, ConstValue, FloatTy, IntTy, Ty, UintTy, UntypedTy};

impl FunctionLowerer {
    pub(super) fn local_expr(&self, node: NodeId, local: LocalId, ty: Ty) -> hir::Expr {
        hir::Expr {
            node,
            kind: hir::ExprKind::Local(local),
            ty,
            category: hir::ValueCategory::Place,
            effects: hir::Effects {
                may_read: true,
                ..hir::Effects::default()
            },
            source: SourceRef::node(node),
        }
    }

    pub(super) fn lower_expr(
        &mut self,
        expr: &ExprSyntax,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if let Some(expected) = expected
            && matches!(expected.underlying(), Ty::Interface(_))
        {
            let value = self.lower_expr_inner(expr, None, false)?;
            return self.coerce_interface_value(value, expected, expr.source);
        }
        self.lower_expr_inner(expr, expected, false)
    }

    pub(super) fn lower_expr_inner(
        &mut self,
        expr: &ExprSyntax,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<hir::Expr, Diagnostic> {
        let node = self.alloc_node(expr.source)?;
        let source = SourceRef::node(node);
        let mut lowered = match &expr.kind {
            ExprSyntaxKind::Literal { .. } => {
                let (ty, value) = eval_constant(expr, &self.constants, source, 0)?;
                hir::Expr {
                    node,
                    kind: hir::ExprKind::Constant(value),
                    ty,
                    category: hir::ValueCategory::Constant,
                    effects: hir::Effects::default(),
                    source,
                }
            }
            ExprSyntaxKind::Ident(ident) => {
                let name = ident.name.as_ref();
                if self.lookup_closure(name).is_some() {
                    return Err(Diagnostic::unsupported(
                        format!(
                            "local function {name} is non-escaping and can only be called directly"
                        ),
                        source,
                    ));
                } else if let Some(local) = self.lookup_local(name) {
                    let ty = self.place_ty(hir::Place::Local(local))?.clone();
                    self.local_expr(node, local, ty)
                } else if let Some(constant) = self.constants.get(name).cloned() {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::GlobalConstant(constant.id, constant.value),
                        ty: constant.ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    }
                } else if let Some(variable) = self.variables.get(name).cloned() {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::GlobalVariable(variable.id, variable.value),
                        ty: variable.ty,
                        category: hir::ValueCategory::Value,
                        effects: hir::Effects {
                            may_read: true,
                            ..hir::Effects::default()
                        },
                        source,
                    }
                } else if self.functions.contains_key(name) {
                    return Err(Diagnostic::unsupported(
                        format!("function value {name} is not implemented by the HIR/MIR backend"),
                        source,
                    ));
                } else if matches!(name, "true" | "false") {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(ConstValue::Bool(name == "true")),
                        ty: Ty::Untyped(UntypedTy::Bool),
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    }
                } else if matches!(name, "print" | "println") {
                    return Err(Diagnostic::unsupported(
                        format!("builtin value {name} is not implemented"),
                        source,
                    ));
                } else {
                    return Err(Diagnostic::semantic(
                        format!("undefined identifier {name}"),
                        source,
                    ));
                }
            }
            ExprSyntaxKind::Paren(expression) => {
                return self.lower_expr_inner(expression, expected, allow_discarded_call_result);
            }
            ExprSyntaxKind::Unary { token, expression } => {
                if *token == Token::ARROW {
                    return self.lower_channel_receive(expression, false, node, source, expected);
                }
                if *token == Token::AND {
                    return self.lower_address_of_local(expression, node, source, expected);
                }
                if *token == Token::MUL {
                    return self.lower_pointer_deref(expression, node, source, expected);
                }
                let mut operand = self.lower_expr(expression, expected)?;
                let operand_ty = operand.ty.default_typed();
                ensure_bootstrap_value_type(&operand_ty, source)?;
                let operator_ty = operand_ty.underlying();
                let op = match *token {
                    Token::ADD if *operator_ty == Ty::Int(IntTy::Int) => hir::UnaryOp::Positive,
                    Token::ADD
                        if matches!(
                            operator_ty,
                            Ty::Float(FloatTy::Float64) | Ty::Complex(ComplexTy::Complex128)
                        ) =>
                    {
                        hir::UnaryOp::Positive
                    }
                    Token::SUB if *operator_ty == Ty::Int(IntTy::Int) => hir::UnaryOp::Negative,
                    Token::SUB
                        if matches!(
                            operator_ty,
                            Ty::Float(FloatTy::Float64) | Ty::Complex(ComplexTy::Complex128)
                        ) =>
                    {
                        hir::UnaryOp::Negative
                    }
                    Token::NOT if is_bool(&operand_ty) => hir::UnaryOp::Not,
                    Token::XOR if *operator_ty == Ty::Int(IntTy::Int) => hir::UnaryOp::BitNot,
                    _ => {
                        return Err(Diagnostic::semantic(
                            format!("invalid unary {token:?} operand {:?}", operand.ty),
                            source,
                        ));
                    }
                };
                if let Some(value) = expr_constant(&operand)
                    .map(|value| fold_constant_unary(op, value, source))
                    .transpose()?
                    .flatten()
                {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(value),
                        ty: operand.ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    }
                } else {
                    coerce_expr(&mut operand, &operand_ty, source)?;
                    let effects = operand.effects;
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Unary {
                            op,
                            operand: Box::new(operand),
                        },
                        ty: operand_ty,
                        category: hir::ValueCategory::Value,
                        effects,
                        source,
                    }
                }
            }
            ExprSyntaxKind::Binary { left, token, right } => {
                if matches!(token, Token::EQL | Token::NEQ)
                    && let Some((arguments, spread)) = recover_nil_comparison(left, right)
                {
                    if spread || !arguments.is_empty() {
                        return Err(Diagnostic::semantic(
                            "recover requires no arguments",
                            source,
                        ));
                    }
                    let equal = *token == Token::EQL;
                    let mut recovered = if !self.inside_deferred_closure {
                        hir::Expr {
                            node,
                            kind: hir::ExprKind::Constant(ConstValue::Bool(equal)),
                            ty: Ty::Bool,
                            category: hir::ValueCategory::Constant,
                            effects: hir::Effects::default(),
                            source,
                        }
                    } else {
                        hir::Expr {
                            node,
                            kind: hir::ExprKind::RecoverCompareNil { equal },
                            ty: Ty::Bool,
                            category: hir::ValueCategory::Value,
                            effects: hir::Effects {
                                may_read: true,
                                may_write: true,
                                ..hir::Effects::default()
                            },
                            source,
                        }
                    };
                    if let Some(expected) = expected {
                        coerce_expr(&mut recovered, expected, source)?;
                    }
                    return Ok(recovered);
                }
                if matches!(token, Token::EQL | Token::NEQ) {
                    let map = if is_nil_identifier(left) {
                        Some(right.as_ref())
                    } else if is_nil_identifier(right) {
                        Some(left.as_ref())
                    } else {
                        None
                    };
                    if let Some(map) = map {
                        return self.lower_nil_comparison(
                            map,
                            *token == Token::EQL,
                            node,
                            source,
                            expected,
                        );
                    }
                }
                let mut left = self.lower_expr(left, None)?;
                let mut right = self.lower_expr(right, None)?;
                if matches!(token, Token::EQL | Token::NEQ)
                    && (left.ty.bootstrap_i64_struct_pointer_fields().is_some()
                        || right.ty.bootstrap_i64_struct_pointer_fields().is_some())
                {
                    return self.lower_struct_pointer_comparison(
                        left,
                        right,
                        *token == Token::EQL,
                        node,
                        source,
                        expected,
                    );
                }
                let op = lower_binary_op(*token).ok_or_else(|| {
                    Diagnostic::unsupported(
                        format!("binary operator {token:?} is not implemented"),
                        source,
                    )
                })?;
                let comparison = matches!(
                    op,
                    hir::BinaryOp::Equal
                        | hir::BinaryOp::NotEqual
                        | hir::BinaryOp::Less
                        | hir::BinaryOp::LessEqual
                        | hir::BinaryOp::Greater
                        | hir::BinaryOp::GreaterEqual
                );
                let logical = matches!(op, hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr);
                let operand_ty = common_operand_type(&left.ty, &right.ty).ok_or_else(|| {
                    Diagnostic::semantic(
                        format!(
                            "incompatible binary operands {:?} and {:?}",
                            left.ty, right.ty
                        ),
                        source,
                    )
                })?;
                coerce_expr(&mut left, &operand_ty, source)?;
                coerce_expr(&mut right, &operand_ty, source)?;
                validate_binary_operator(op, &operand_ty, source)?;
                let mut effects = left.effects.union(right.effects);
                if matches!(
                    op,
                    hir::BinaryOp::Div
                        | hir::BinaryOp::Rem
                        | hir::BinaryOp::Shl
                        | hir::BinaryOp::Shr
                ) {
                    effects.may_panic = true;
                }
                if op == hir::BinaryOp::Add && operand_ty == Ty::String {
                    effects.may_allocate = true;
                }
                let result_ty = if comparison || logical {
                    Ty::Bool
                } else {
                    operand_ty
                };
                let folded = expr_constant(&left)
                    .zip(expr_constant(&right))
                    .map(|(left, right)| fold_constant_binary(op, left, right, source))
                    .transpose()?
                    .flatten();
                if let Some(value) = folded {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(value),
                        ty: result_ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    }
                } else {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Binary {
                            op,
                            left: Box::new(left),
                            right: Box::new(right),
                        },
                        ty: result_ty,
                        category: hir::ValueCategory::Value,
                        effects,
                        source,
                    }
                }
            }
            ExprSyntaxKind::Call {
                callee,
                arguments,
                spread,
            } => {
                if let ExprSyntaxKind::Selector { base, member } = &callee.kind {
                    return self.lower_selector_call(
                        base,
                        member,
                        arguments,
                        *spread,
                        node,
                        source,
                        expected,
                        allow_discarded_call_result,
                    );
                }
                let ExprSyntaxKind::Ident(callee_ident) = &callee.kind else {
                    return Err(Diagnostic::unsupported(
                        "only direct calls are implemented by the HIR/MIR backend",
                        source,
                    ));
                };
                let name = callee_ident.name.as_ref();
                if name == "make" {
                    return self
                        .lower_make_builtin_call(arguments, *spread, node, source, expected);
                }
                if name == "new" {
                    return self.lower_new_builtin_call(arguments, *spread, node, source, expected);
                }
                if name == "len" {
                    return self.lower_len_builtin_call(arguments, *spread, node, source, expected);
                }
                if name == "cap" {
                    return self.lower_channel_cap_builtin_call(
                        arguments, *spread, node, source, expected,
                    );
                }
                if name == "close" {
                    return self.lower_channel_close_builtin_call(
                        arguments, *spread, node, source, expected,
                    );
                }
                if name == "clear" {
                    return self
                        .lower_clear_builtin_call(arguments, *spread, node, source, expected);
                }
                if name == "delete" {
                    return self
                        .lower_delete_builtin_call(arguments, *spread, node, source, expected);
                }
                if matches!(name, "append" | "copy") {
                    return self.lower_slice_builtin_call(
                        name, arguments, *spread, node, source, expected,
                    );
                }
                if matches!(name, "min" | "max" | "complex" | "real" | "imag") {
                    return self.lower_numeric_builtin_call(
                        name, arguments, *spread, node, source, expected,
                    );
                }
                if self.type_aliases.contains_key(name)
                    || matches!(name, "bool" | "string" | "int" | "float64" | "complex128")
                {
                    let [argument] = arguments.as_ref() else {
                        return Err(Diagnostic::semantic(
                            format!("conversion to {name} requires exactly one argument"),
                            source,
                        ));
                    };
                    let target = lower_type(callee, &self.type_aliases, source)?;
                    let mut argument = self.lower_expr(argument, None)?;
                    if target == Ty::String
                        && argument.ty == Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8)))
                    {
                        let effects = slice_runtime_effects(&[&argument], false, true, false);
                        return Ok(hir::Expr {
                            node,
                            kind: hir::ExprKind::Call {
                                callee: hir::Callee::Builtin(hir::Builtin::StringFromSliceU8),
                                args: vec![argument],
                            },
                            ty: Ty::String,
                            category: hir::ValueCategory::Value,
                            effects,
                            source,
                        });
                    }
                    if is_assignable(&argument.ty, &target) {
                        coerce_expr(&mut argument, &target, source)?;
                    } else if argument.ty.underlying() == target.underlying() {
                        let effects = argument.effects;
                        return Ok(hir::Expr {
                            node,
                            kind: hir::ExprKind::Conversion {
                                value: Box::new(argument),
                            },
                            ty: target,
                            category: hir::ValueCategory::Value,
                            effects,
                            source,
                        });
                    } else {
                        return Err(Diagnostic::unsupported(
                            format!(
                                "conversion from {:?} to {name} requires a representation change",
                                argument.ty
                            ),
                            source,
                        ));
                    }
                    return Ok(argument);
                }
                let (callee, params, results, variadic) = if let Some(id) =
                    self.lookup_closure(name)
                {
                    let closure = self.closures.get(id.index() as usize).ok_or_else(|| {
                        Diagnostic::backend(format!("unknown local function {name}"))
                    })?;
                    (
                        hir::Callee::Closure(id),
                        closure.signature.params.clone(),
                        closure.signature.results.clone(),
                        closure.signature.variadic,
                    )
                } else if self.lookup_local(name).is_some() {
                    return Err(Diagnostic::unsupported(
                        format!(
                            "calling local value {name} requires function-value HIR and is not implemented"
                        ),
                        source,
                    ));
                } else if let Some(symbol) = self.functions.get(name).cloned() {
                    (
                        hir::Callee::Function(symbol.id),
                        symbol.signature.params,
                        symbol.signature.results,
                        symbol.signature.variadic,
                    )
                } else if self.constants.contains_key(name) {
                    return Err(Diagnostic::semantic(
                        format!("constant {name} is not callable"),
                        source,
                    ));
                } else {
                    match name {
                        "print" => (
                            hir::Callee::Builtin(hir::Builtin::Print),
                            vec![],
                            vec![],
                            false,
                        ),
                        "println" => (
                            hir::Callee::Builtin(hir::Builtin::Println),
                            vec![],
                            vec![],
                            false,
                        ),
                        "panic" => (
                            hir::Callee::Builtin(hir::Builtin::Panic),
                            vec![],
                            vec![],
                            false,
                        ),
                        "recover" => {
                            return Err(Diagnostic::unsupported(
                                "recover results are currently supported in direct nil comparisons",
                                source,
                            ));
                        }
                        name => {
                            return Err(Diagnostic::semantic(
                                format!("undefined function {name}"),
                                source,
                            ));
                        }
                    }
                };
                match callee {
                    hir::Callee::Function(_) | hir::Callee::Closure(_) if *spread && !variadic => {
                        return Err(Diagnostic::semantic(
                            "... is only valid when calling a variadic function",
                            source,
                        ));
                    }
                    hir::Callee::Function(_) | hir::Callee::Closure(_) if variadic && !*spread => {
                        return Err(Diagnostic::unsupported(
                            "individual variadic arguments require slice-pack lowering",
                            source,
                        ));
                    }
                    hir::Callee::Function(_) | hir::Callee::Closure(_)
                        if arguments.len() != params.len() =>
                    {
                        return Err(Diagnostic::semantic(
                            format!(
                                "call has {} arguments; expected {}",
                                arguments.len(),
                                params.len()
                            ),
                            source,
                        ));
                    }
                    hir::Callee::Builtin(hir::Builtin::Panic) if arguments.len() != 1 => {
                        return Err(Diagnostic::semantic(
                            format!(
                                "call to panic has {} arguments; expected 1",
                                arguments.len()
                            ),
                            source,
                        ));
                    }
                    hir::Callee::Builtin(_) if *spread => {
                        return Err(Diagnostic::semantic(
                            "... is not valid for this built-in call",
                            source,
                        ));
                    }
                    _ => {}
                }
                let args = if matches!(callee, hir::Callee::Builtin(_)) {
                    arguments
                        .iter()
                        .map(|argument| {
                            self.lower_expr(argument, None).and_then(|expression| {
                                let expression_source = expression.source;
                                default_expr_type(expression, expression_source)
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    arguments
                        .iter()
                        .zip(&params)
                        .map(|(argument, expected)| self.lower_expr(argument, Some(expected)))
                        .collect::<Result<Vec<_>, _>>()?
                };
                let ty = match results.as_slice() {
                    [] => Ty::Unit,
                    [single] => single.clone(),
                    many => Ty::Tuple(many.to_vec()),
                };
                if ty == Ty::Unit && !allow_discarded_call_result {
                    return Err(Diagnostic::unsupported(
                        "a no-result call cannot be used as a value",
                        source,
                    ));
                }
                let effects = args.iter().fold(
                    hir::Effects {
                        may_read: false,
                        may_call: true,
                        may_allocate: true,
                        may_block: true,
                        may_panic: true,
                        may_write: true,
                    },
                    |effects, argument| effects.union(argument.effects),
                );
                hir::Expr {
                    node,
                    kind: hir::ExprKind::Call { callee, args },
                    ty,
                    category: hir::ValueCategory::Value,
                    effects,
                    source,
                }
            }
            ExprSyntaxKind::FunctionLiteral { .. } => {
                return Err(Diagnostic::unsupported(
                    "function literals currently require a non-escaping short declaration",
                    source,
                ));
            }
            ExprSyntaxKind::Selector { base, member } => {
                self.lower_selector(base, member, node, source)?
            }
            ExprSyntaxKind::CompositeLiteral { ty, elements } => self.lower_composite_literal(
                ty.as_deref(),
                elements,
                node,
                expr.source,
                source,
                expected,
            )?,
            ExprSyntaxKind::Index { base, index } => {
                let base = self.lower_expr(base, None)?;
                if matches!(base.ty.underlying(), Ty::Map(_, _)) {
                    return self.lower_map_index(base, index, node, source, expected);
                }
                if matches!(base.ty.underlying(), Ty::Array(_, _)) {
                    return self.lower_array_index(base, index, node, source, expected);
                }
                let Ty::Slice(element) = base.ty.underlying() else {
                    return Err(Diagnostic::semantic(
                        "indexing requires a slice value",
                        source,
                    ));
                };
                let builtin = if element.underlying() == &Ty::Int(IntTy::Int) {
                    hir::Builtin::SliceI64Index
                } else if element.underlying() == &Ty::Bool {
                    hir::Builtin::SliceBoolIndex
                } else {
                    return Err(Diagnostic::unsupported(
                        "indexing currently supports []bool and []int values",
                        source,
                    ));
                };
                let element_ty = element.as_ref().clone();
                let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
                let effects = slice_runtime_effects(&[&base, &index], false, false, true);
                hir::Expr {
                    node,
                    kind: hir::ExprKind::Call {
                        callee: hir::Callee::Builtin(builtin),
                        args: vec![base, index],
                    },
                    ty: element_ty,
                    category: hir::ValueCategory::Value,
                    effects,
                    source,
                }
            }
            ExprSyntaxKind::Slice {
                base,
                low,
                high,
                max,
            } => {
                let base = self.lower_expr(base, None)?;
                let Ty::Slice(element) = base.ty.underlying() else {
                    return Err(Diagnostic::semantic(
                        "slicing requires a slice value",
                        source,
                    ));
                };
                if element.underlying() != &Ty::Int(IntTy::Int) {
                    return Err(Diagnostic::unsupported(
                        "reslicing currently supports []int values",
                        source,
                    ));
                }
                let slice_ty = Ty::Slice(element.clone());
                let low = self.lower_optional_slice_bound(low.as_deref(), expr.source)?;
                let high = self.lower_optional_slice_bound(high.as_deref(), expr.source)?;
                let max = self.lower_optional_slice_bound(max.as_deref(), expr.source)?;
                let effects =
                    slice_runtime_effects(&[&base, &low, &high, &max], false, false, true);
                hir::Expr {
                    node,
                    kind: hir::ExprKind::Call {
                        callee: hir::Callee::Builtin(hir::Builtin::SliceI64Range),
                        args: vec![base, low, high, max],
                    },
                    ty: slice_ty,
                    category: hir::ValueCategory::Value,
                    effects,
                    source,
                }
            }
            ExprSyntaxKind::ArrayType { .. }
            | ExprSyntaxKind::MapType { .. }
            | ExprSyntaxKind::ChannelType { .. }
            | ExprSyntaxKind::FunctionType { .. }
            | ExprSyntaxKind::StructType { .. }
            | ExprSyntaxKind::InterfaceType { .. } => {
                return Err(Diagnostic::semantic(
                    "a type is not a value expression",
                    source,
                ));
            }
            ExprSyntaxKind::KeyValue { .. } => {
                return Err(Diagnostic::semantic(
                    "key: value syntax requires a composite literal",
                    source,
                ));
            }
            ExprSyntaxKind::Unsupported(kind) => {
                return Err(Diagnostic::unsupported(
                    format!("expression {kind} is not implemented by the HIR/MIR backend"),
                    source,
                ));
            }
        };
        if let Some(expected) = expected {
            coerce_expr(&mut lowered, expected, source)?;
        }
        Ok(lowered)
    }

    fn lower_optional_slice_bound(
        &mut self,
        bound: Option<&ExprSyntax>,
        syntax_source: crate::compiler::syntax::SyntaxSource,
    ) -> Result<hir::Expr, Diagnostic> {
        if let Some(bound) = bound {
            return self.lower_expr(bound, Some(&Ty::Int(IntTy::Int)));
        }
        let node = self.alloc_node(syntax_source)?;
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::Constant(ConstValue::Int("-1".into())),
            ty: Ty::Int(IntTy::Int),
            category: hir::ValueCategory::Constant,
            effects: hir::Effects::default(),
            source: SourceRef::node(node),
        })
    }

    pub(super) fn lower_slice_builtin_call(
        &mut self,
        name: &str,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let slice_ty = Ty::Slice(Box::new(Ty::Int(IntTy::Int)));
        let byte_slice_ty = Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8)));
        let (builtin, args, ty, allocates, writes, panics) = match name {
            "make" => {
                let (declared_syntax, len_syntax, cap_syntax) = match arguments {
                    [declared, len] => (declared, len, None),
                    [declared, len, cap] => (declared, len, Some(cap)),
                    _ => {
                        return Err(Diagnostic::semantic(
                            "make([]int, len[, cap]) requires two or three arguments",
                            source,
                        ));
                    }
                };
                let declared = lower_type(declared_syntax, &self.type_aliases, source)?;
                if declared != slice_ty {
                    return Err(Diagnostic::unsupported(
                        "make currently supports []int values",
                        source,
                    ));
                }
                let len = self.lower_expr(len_syntax, Some(&Ty::Int(IntTy::Int)))?;
                let cap = if let Some(cap) = cap_syntax {
                    self.lower_expr(cap, Some(&Ty::Int(IntTy::Int)))?
                } else {
                    self.lower_optional_slice_bound(None, declared_syntax.source)?
                };
                (
                    hir::Builtin::SliceI64Make,
                    vec![len, cap],
                    slice_ty,
                    true,
                    false,
                    true,
                )
            }
            "cap" => {
                let [value] = arguments else {
                    return Err(Diagnostic::semantic(
                        "cap requires exactly one argument",
                        source,
                    ));
                };
                let value = self.lower_expr(value, Some(&slice_ty))?;
                (
                    hir::Builtin::SliceI64Cap,
                    vec![value],
                    Ty::Int(IntTy::Int),
                    false,
                    false,
                    false,
                )
            }
            "append" => {
                let [slice, value] = arguments else {
                    return Err(Diagnostic::semantic(
                        "append currently requires one []int value and one int element",
                        source,
                    ));
                };
                if spread {
                    let slice = self.lower_expr(slice, Some(&byte_slice_ty))?;
                    let mut value = self.lower_expr(value, None)?;
                    if value.ty == Ty::Untyped(UntypedTy::String) {
                        coerce_expr(&mut value, &Ty::String, source)?;
                    }
                    let builtin = match value.ty {
                        Ty::String => hir::Builtin::SliceU8AppendString,
                        ref ty if ty == &byte_slice_ty => hir::Builtin::SliceU8AppendSlice,
                        _ => {
                            return Err(Diagnostic::semantic(
                                "[]byte append spread requires a string or []byte source",
                                source,
                            ));
                        }
                    };
                    (
                        builtin,
                        vec![slice, value],
                        byte_slice_ty,
                        true,
                        true,
                        false,
                    )
                } else {
                    let slice = self.lower_expr(slice, Some(&slice_ty))?;
                    let value = self.lower_expr(value, Some(&Ty::Int(IntTy::Int)))?;
                    (
                        hir::Builtin::SliceI64Append,
                        vec![slice, value],
                        slice_ty,
                        true,
                        true,
                        false,
                    )
                }
            }
            "copy" => {
                let [destination, source_value] = arguments else {
                    return Err(Diagnostic::semantic(
                        "copy currently requires a []byte destination and string source",
                        source,
                    ));
                };
                if spread {
                    return Err(Diagnostic::semantic("copy does not accept ...", source));
                }
                let destination = self.lower_expr(destination, None)?;
                let (builtin, source_value) = if destination.ty == slice_ty {
                    (
                        hir::Builtin::SliceI64Copy,
                        self.lower_expr(source_value, Some(&slice_ty))?,
                    )
                } else if destination.ty == byte_slice_ty {
                    (
                        hir::Builtin::SliceU8CopyString,
                        self.lower_expr(source_value, Some(&Ty::String))?,
                    )
                } else {
                    return Err(Diagnostic::semantic(
                        "copy currently supports []int slices or a []byte destination and string source",
                        source,
                    ));
                };
                (
                    builtin,
                    vec![destination, source_value],
                    Ty::Int(IntTy::Int),
                    false,
                    true,
                    false,
                )
            }
            _ => {
                return Err(Diagnostic::backend(
                    "non-slice builtin reached slice lowering",
                ));
            }
        };
        let argument_refs = args.iter().collect::<Vec<_>>();
        let effects = slice_runtime_effects(&argument_refs, writes, allocates, panics);
        let mut lowered = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(builtin),
                args,
            },
            ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut lowered, expected, source)?;
        }
        Ok(lowered)
    }
}

fn recover_nil_comparison<'a>(
    left: &'a ExprSyntax,
    right: &'a ExprSyntax,
) -> Option<(&'a [ExprSyntax], bool)> {
    recover_call(left)
        .filter(|_| is_nil_identifier(right))
        .or_else(|| recover_call(right).filter(|_| is_nil_identifier(left)))
}

fn recover_call(expression: &ExprSyntax) -> Option<(&[ExprSyntax], bool)> {
    let ExprSyntaxKind::Call {
        callee,
        arguments,
        spread,
    } = &expression.kind
    else {
        return None;
    };
    let ExprSyntaxKind::Ident(callee) = &callee.kind else {
        return None;
    };
    (callee.name.as_ref() == "recover").then_some((arguments, *spread))
}

fn is_nil_identifier(expression: &ExprSyntax) -> bool {
    matches!(
        &expression.kind,
        ExprSyntaxKind::Ident(identifier) if identifier.name.as_ref() == "nil"
    )
}

fn slice_runtime_effects(
    arguments: &[&hir::Expr],
    writes: bool,
    allocates: bool,
    panics: bool,
) -> hir::Effects {
    arguments.iter().fold(
        hir::Effects {
            may_call: true,
            may_allocate: allocates,
            may_panic: panics,
            may_write: writes,
            ..hir::Effects::default()
        },
        |effects, argument| effects.union(argument.effects),
    )
}
