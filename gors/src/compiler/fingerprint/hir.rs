//! Canonical encoding of typed HIR.

mod builtins;
mod static_values;

use super::Fingerprint;
use super::encoder::{
    Encoder, closure_id, const_value, def_id, hir_effects, local_id, node_id, package_id,
    qualified_def_id, signature, source_ref, ty,
};
use crate::compiler::hir;
use builtins::encode_builtin;
use static_values::encode_static_value;

/// Fingerprint a complete typed HIR file, including declaration order.
#[must_use]
pub fn hir_file(file: &hir::File) -> Fingerprint {
    let mut encoder = Encoder::root(b"hir-file");
    encode_file(&mut encoder, file);
    encoder.finish()
}

/// Fingerprint one typed HIR function independently of sibling declarations.
#[must_use]
pub fn hir_function(function: &hir::Function) -> Fingerprint {
    let mut encoder = Encoder::root(b"hir-function");
    encode_function(&mut encoder, function);
    encoder.finish()
}

/// Fingerprint one exact typed package constant independently of siblings.
#[must_use]
pub fn hir_constant(constant: &hir::Constant) -> Fingerprint {
    let mut encoder = Encoder::root(b"hir-constant");
    encode_constant(&mut encoder, constant);
    encoder.finish()
}

/// Fingerprint one typed immutable package variable independently of siblings.
#[must_use]
pub fn hir_variable(variable: &hir::Variable) -> Fingerprint {
    let mut encoder = Encoder::root(b"hir-variable");
    encode_variable(&mut encoder, variable);
    encoder.finish()
}

fn encode_file(encoder: &mut Encoder, file: &hir::File) {
    encoder.field(b"package-id", |encoder| {
        package_id(encoder, file.package_id)
    });
    encoder.field(b"package", |encoder| encoder.string(&file.package));
    encoder.field(b"constants", |encoder| {
        encoder.sequence(&file.constants, encode_constant);
    });
    encoder.field(b"functions", |encoder| {
        encoder.sequence(&file.functions, encode_function);
    });
}

fn encode_constant(encoder: &mut Encoder, constant: &hir::Constant) {
    encoder.field(b"id", |encoder| def_id(encoder, constant.id));
    encoder.field(b"name", |encoder| encoder.string(&constant.name));
    encoder.field(b"type", |encoder| ty(encoder, &constant.ty));
    encoder.field(b"value", |encoder| const_value(encoder, &constant.value));
    encoder.field(b"source", |encoder| source_ref(encoder, constant.source));
}

fn encode_variable(encoder: &mut Encoder, variable: &hir::Variable) {
    encoder.field(b"id", |encoder| def_id(encoder, variable.id));
    encoder.field(b"name", |encoder| encoder.string(&variable.name));
    encoder.field(b"type", |encoder| ty(encoder, &variable.ty));
    encoder.field(b"value", |encoder| {
        encode_static_value(encoder, &variable.value);
    });
    encoder.field(b"source", |encoder| source_ref(encoder, variable.source));
}

fn encode_function(encoder: &mut Encoder, function: &hir::Function) {
    encoder.field(b"id", |encoder| def_id(encoder, function.id));
    encoder.field(b"node", |encoder| node_id(encoder, function.node));
    encoder.field(b"name", |encoder| encoder.string(&function.name));
    encoder.field(b"signature", |encoder| {
        signature(encoder, &function.signature);
    });
    encoder.field(b"parameters", |encoder| {
        encoder.sequence(&function.params, |encoder, id| local_id(encoder, *id));
    });
    encoder.field(b"named-results", |encoder| {
        encoder.sequence(&function.named_results, |encoder, result| {
            encoder.option(result.as_ref(), |encoder, id| local_id(encoder, *id));
        });
    });
    encoder.field(b"locals", |encoder| {
        encoder.sequence(&function.locals, encode_local);
    });
    encoder.field(b"closures", |encoder| {
        encoder.sequence(&function.closures, encode_closure);
    });
    encoder.field(b"body", |encoder| {
        encode_block(encoder, &function.body);
    });
    encoder.field(b"source", |encoder| source_ref(encoder, function.source));
}

fn encode_closure(encoder: &mut Encoder, closure: &hir::Closure) {
    encoder.field(b"id", |encoder| closure_id(encoder, closure.id));
    encoder.field(b"signature", |encoder| {
        signature(encoder, &closure.signature);
    });
    encoder.field(b"parameters", |encoder| {
        encoder.sequence(&closure.params, |encoder, id| local_id(encoder, *id));
    });
    encoder.field(b"named-results", |encoder| {
        encoder.sequence(&closure.named_results, |encoder, result| {
            encoder.option(result.as_ref(), |encoder, id| local_id(encoder, *id));
        });
    });
    encoder.field(b"body", |encoder| encode_block(encoder, &closure.body));
    encoder.field(b"source", |encoder| source_ref(encoder, closure.source));
}

fn encode_local(encoder: &mut Encoder, local: &hir::Local) {
    encoder.field(b"id", |encoder| local_id(encoder, local.id));
    encoder.field(b"name", |encoder| {
        encoder.option(local.name.as_ref(), |encoder, name| encoder.string(name));
    });
    encoder.field(b"type", |encoder| ty(encoder, &local.ty));
    encoder.field(b"kind", |encoder| encode_local_kind(encoder, local.kind));
    encoder.field(b"source", |encoder| source_ref(encoder, local.source));
}

fn encode_local_kind(encoder: &mut Encoder, kind: hir::LocalKind) {
    encoder.variant(
        match kind {
            hir::LocalKind::Parameter => b"parameter",
            hir::LocalKind::NamedResult => b"named-result",
            hir::LocalKind::Variable => b"variable",
            hir::LocalKind::Temporary => b"temporary",
        },
        |_| {},
    );
}

fn encode_block(encoder: &mut Encoder, block: &hir::Block) {
    encoder.field(b"node", |encoder| node_id(encoder, block.node));
    encoder.field(b"statements", |encoder| {
        encoder.sequence(&block.stmts, encode_statement);
    });
    encoder.field(b"source", |encoder| source_ref(encoder, block.source));
}

fn encode_statement(encoder: &mut Encoder, statement: &hir::Stmt) {
    encoder.field(b"node", |encoder| node_id(encoder, statement.node));
    encoder.field(b"kind", |encoder| {
        encode_statement_kind(encoder, &statement.kind);
    });
    encoder.field(b"source", |encoder| source_ref(encoder, statement.source));
}

fn encode_statement_kind(encoder: &mut Encoder, kind: &hir::StmtKind) {
    match kind {
        hir::StmtKind::Let {
            destinations,
            values,
        } => encoder.variant(b"let", |encoder| {
            encode_places_and_values(encoder, destinations, values);
        }),
        hir::StmtKind::LetTuple {
            destinations,
            value,
            coercions,
        } => encoder.variant(b"let-tuple", |encoder| {
            encoder.field(b"destinations", |encoder| {
                encoder.sequence(destinations, |encoder, place| encode_place(encoder, *place));
            });
            encoder.field(b"value", |encoder| encode_expression(encoder, value));
            encoder.field(b"coercions", |encoder| {
                encoder.sequence(coercions, encode_value_coercion);
            });
        }),
        hir::StmtKind::Assign {
            destinations,
            op,
            values,
        } => encoder.variant(b"assign", |encoder| {
            encoder.field(b"destinations", |encoder| {
                encoder.sequence(destinations, encode_assignment_target);
            });
            encoder.field(b"operation", |encoder| encode_assign_op(encoder, *op));
            encoder.field(b"values", |encoder| {
                encoder.sequence(values, |encoder, expression| {
                    encode_expression(encoder, expression);
                });
            });
        }),
        hir::StmtKind::AssignTuple {
            destinations,
            value,
            coercions,
        } => encoder.variant(b"assign-tuple", |encoder| {
            encoder.field(b"destinations", |encoder| {
                encoder.sequence(destinations, encode_assignment_target);
            });
            encoder.field(b"value", |encoder| encode_expression(encoder, value));
            encoder.field(b"coercions", |encoder| {
                encoder.sequence(coercions, encode_value_coercion);
            });
        }),
        hir::StmtKind::Expr(expression) => {
            encoder.variant(b"expression", |encoder| {
                encode_expression(encoder, expression)
            });
        }
        hir::StmtKind::ClosureBinding(id) => {
            encoder.variant(b"closure-binding", |encoder| closure_id(encoder, *id));
        }
        hir::StmtKind::Defer {
            parameters,
            values,
            body,
        } => encoder.variant(b"defer", |encoder| {
            encoder.field(b"parameters", |encoder| {
                encoder.sequence(parameters, |encoder, parameter| {
                    local_id(encoder, *parameter);
                });
            });
            encoder.field(b"values", |encoder| {
                encoder.sequence(values, |encoder, expression| {
                    encode_expression(encoder, expression);
                });
            });
            encoder.field(b"body", |encoder| encode_block(encoder, body));
        }),
        hir::StmtKind::Go {
            parameters,
            values,
            body,
        } => encoder.variant(b"go", |encoder| {
            encoder.field(b"parameters", |encoder| {
                encoder.sequence(parameters, |encoder, parameter| {
                    local_id(encoder, *parameter);
                });
            });
            encoder.field(b"values", |encoder| {
                encoder.sequence(values, |encoder, expression| {
                    encode_expression(encoder, expression);
                });
            });
            encoder.field(b"body", |encoder| encode_block(encoder, body));
        }),
        hir::StmtKind::Return(values) => encoder.variant(b"return", |encoder| {
            encoder.sequence(values, |encoder, expression| {
                encode_expression(encoder, expression);
            });
        }),
        hir::StmtKind::If {
            init,
            condition,
            then_block,
            else_branch,
        } => encoder.variant(b"if", |encoder| {
            encoder.field(b"init", |encoder| {
                encoder.option(init.as_deref(), |encoder, statement| {
                    encode_statement(encoder, statement);
                });
            });
            encoder.field(b"condition", |encoder| {
                encode_expression(encoder, condition);
            });
            encoder.field(b"then", |encoder| {
                encode_block(encoder, then_block);
            });
            encoder.field(b"else", |encoder| {
                encoder.option(else_branch.as_deref(), |encoder, statement| {
                    encode_statement(encoder, statement);
                });
            });
        }),
        hir::StmtKind::For {
            target,
            label,
            init,
            condition,
            post,
            body,
        } => encoder.variant(b"for", |encoder| {
            encoder.field(b"target", |encoder| encoder.u32(target.index()));
            encoder.field(b"label", |encoder| {
                encoder.option(label.as_ref(), |encoder, label| encoder.string(label));
            });
            encoder.field(b"init", |encoder| {
                encoder.option(init.as_deref(), |encoder, statement| {
                    encode_statement(encoder, statement);
                });
            });
            encoder.field(b"condition", |encoder| {
                encoder.option(condition.as_ref(), |encoder, expression| {
                    encode_expression(encoder, expression);
                });
            });
            encoder.field(b"post", |encoder| {
                encoder.option(post.as_deref(), |encoder, statement| {
                    encode_statement(encoder, statement);
                });
            });
            encoder.field(b"body", |encoder| {
                encode_block(encoder, body);
            });
        }),
        hir::StmtKind::Range {
            target,
            label,
            bindings,
            expression,
            body,
        } => encoder.variant(b"range", |encoder| {
            encoder.field(b"target", |encoder| encoder.u32(target.index()));
            encoder.field(b"label", |encoder| {
                encoder.option(label.as_ref(), |encoder, label| encoder.string(label));
            });
            encoder.field(b"bindings", |encoder| {
                encode_range_bindings(encoder, bindings)
            });
            encoder.field(b"expression", |encoder| {
                encode_expression(encoder, expression);
            });
            encoder.field(b"body", |encoder| encode_block(encoder, body));
        }),
        hir::StmtKind::Block(block) => {
            encoder.variant(b"block", |encoder| {
                encode_block(encoder, block);
            });
        }
        hir::StmtKind::Breakable { target, body } => {
            encoder.variant(b"breakable", |encoder| {
                encoder.field(b"target", |encoder| encoder.u32(target.index()));
                encoder.field(b"body", |encoder| encode_block(encoder, body));
            });
        }
        hir::StmtKind::Label { name, statement } => encoder.variant(b"label", |encoder| {
            encoder.field(b"name", |encoder| encoder.string(name));
            encoder.field(b"statement", |encoder| {
                encoder.option(statement.as_deref(), |encoder, statement| {
                    encode_statement(encoder, statement);
                });
            });
        }),
        hir::StmtKind::Goto(label) => encoder.variant(b"goto", |encoder| encoder.string(label)),
        hir::StmtKind::Break(target) => {
            encoder.variant(b"break", |encoder| encoder.u32(target.index()));
        }
        hir::StmtKind::Continue(target) => {
            encoder.variant(b"continue", |encoder| encoder.u32(target.index()));
        }
    }
}

fn encode_value_coercion(encoder: &mut Encoder, coercion: &hir::ValueCoercion) {
    match coercion {
        hir::ValueCoercion::Identity => encoder.variant(b"identity", |_| {}),
        hir::ValueCoercion::Representation { target } => {
            encoder.variant(b"representation", |encoder| ty(encoder, target));
        }
        hir::ValueCoercion::Interface {
            target,
            type_identity,
        } => encoder.variant(b"interface", |encoder| {
            encoder.field(b"target", |encoder| ty(encoder, target));
            encoder.field(b"type-identity", |encoder| encoder.blob(type_identity));
        }),
    }
}

fn encode_assignment_target(encoder: &mut Encoder, target: &hir::AssignTarget) {
    encoder.field(b"source", |encoder| source_ref(encoder, target.source));
    encoder.field(b"type", |encoder| {
        encoder.option(target.ty.as_ref(), ty);
    });
    match &target.kind {
        hir::AssignTargetKind::Local(id) => {
            encoder.variant(b"local", |encoder| local_id(encoder, *id));
        }
        hir::AssignTargetKind::Discard => encoder.variant(b"discard", |_| {}),
        hir::AssignTargetKind::SliceIndex { slice, index, set } => {
            encoder.variant(b"slice-index", |encoder| {
                encoder.field(b"slice", |encoder| encode_expression(encoder, slice));
                encoder.field(b"index", |encoder| encode_expression(encoder, index));
                encoder.field(b"set", |encoder| encode_builtin(encoder, *set));
            });
        }
        hir::AssignTargetKind::MapIndex { map, key } => {
            encoder.variant(b"map-index", |encoder| {
                encoder.field(b"map", |encoder| encode_expression(encoder, map));
                encoder.field(b"key", |encoder| encode_expression(encoder, key));
            });
        }
        hir::AssignTargetKind::ArrayIndex { array, index } => {
            encoder.variant(b"array-index", |encoder| {
                encoder.field(b"array", |encoder| local_id(encoder, *array));
                encoder.field(b"index", |encoder| encode_expression(encoder, index));
            });
        }
        hir::AssignTargetKind::Pointer { pointer, set } => {
            encoder.variant(b"pointer", |encoder| {
                encoder.field(b"pointer", |encoder| encode_expression(encoder, pointer));
                encoder.field(b"set", |encoder| encode_builtin(encoder, *set));
            });
        }
        hir::AssignTargetKind::StructFieldPath { structure, fields } => {
            encoder.variant(b"struct-field-path", |encoder| {
                encoder.field(b"structure", |encoder| local_id(encoder, *structure));
                encoder.field(b"fields", |encoder| {
                    encoder.sequence(fields, |encoder, field| encoder.u32(*field));
                });
            });
        }
        hir::AssignTargetKind::PointerStructField {
            pointer,
            field,
            set,
        } => encoder.variant(b"pointer-struct-field", |encoder| {
            encoder.field(b"pointer", |encoder| encode_expression(encoder, pointer));
            encoder.field(b"field", |encoder| encoder.u32(*field));
            encoder.field(b"set", |encoder| encode_builtin(encoder, *set));
        }),
    }
}

fn encode_range_bindings(encoder: &mut Encoder, bindings: &hir::RangeBindings) {
    match bindings {
        hir::RangeBindings::Declared { key, value } => {
            encoder.variant(b"declared", |encoder| {
                encoder.field(b"key", |encoder| {
                    encoder.option(key.as_ref(), |encoder, place| encode_place(encoder, *place));
                });
                encoder.field(b"value", |encoder| {
                    encoder.option(value.as_ref(), |encoder, place| {
                        encode_place(encoder, *place)
                    });
                });
            });
        }
        hir::RangeBindings::Assigned { targets, coercions } => {
            encoder.variant(b"assigned", |encoder| {
                encoder.field(b"targets", |encoder| {
                    encoder.sequence(targets, encode_assignment_target);
                });
                encoder.field(b"coercions", |encoder| {
                    encoder.sequence(coercions, encode_value_coercion);
                });
            });
        }
    }
}

fn encode_places_and_values(
    encoder: &mut Encoder,
    destinations: &[hir::Place],
    values: &[hir::Expr],
) {
    encoder.field(b"destinations", |encoder| {
        encoder.sequence(destinations, |encoder, place| encode_place(encoder, *place));
    });
    encoder.field(b"values", |encoder| {
        encoder.sequence(values, |encoder, expression| {
            encode_expression(encoder, expression);
        });
    });
}

fn encode_place(encoder: &mut Encoder, place: hir::Place) {
    match place {
        hir::Place::Local(id) => {
            encoder.variant(b"local", |encoder| local_id(encoder, id));
        }
        hir::Place::Discard => encoder.variant(b"discard", |_| {}),
    }
}

fn encode_assign_op(encoder: &mut Encoder, op: hir::AssignOp) {
    encoder.variant(
        match op {
            hir::AssignOp::Set => b"set",
            hir::AssignOp::Add => b"add",
            hir::AssignOp::Sub => b"subtract",
            hir::AssignOp::Mul => b"multiply",
            hir::AssignOp::Div => b"divide",
            hir::AssignOp::Rem => b"remainder",
            hir::AssignOp::BitAnd => b"bit-and",
            hir::AssignOp::BitOr => b"bit-or",
            hir::AssignOp::BitXor => b"bit-xor",
            hir::AssignOp::Shl => b"shift-left",
            hir::AssignOp::Shr => b"shift-right",
            hir::AssignOp::AndNot => b"and-not",
        },
        |_| {},
    );
}

fn encode_expression(encoder: &mut Encoder, expression: &hir::Expr) {
    encoder.field(b"node", |encoder| node_id(encoder, expression.node));
    encoder.field(b"kind", |encoder| {
        encode_expression_kind(encoder, &expression.kind);
    });
    encoder.field(b"type", |encoder| ty(encoder, &expression.ty));
    encoder.field(b"category", |encoder| {
        encode_value_category(encoder, expression.category);
    });
    encoder.field(b"effects", |encoder| {
        hir_effects(encoder, expression.effects);
    });
    encoder.field(b"source", |encoder| source_ref(encoder, expression.source));
}

fn encode_expression_kind(encoder: &mut Encoder, kind: &hir::ExprKind) {
    match kind {
        hir::ExprKind::Constant(value) => {
            encoder.variant(b"constant", |encoder| const_value(encoder, value));
        }
        hir::ExprKind::Local(id) => {
            encoder.variant(b"local", |encoder| local_id(encoder, *id));
        }
        hir::ExprKind::AddressOfLocal(id) => {
            encoder.variant(b"address-of-local", |encoder| local_id(encoder, *id));
        }
        hir::ExprKind::AddressOfValue(value) => {
            encoder.variant(b"address-of-value", |encoder| {
                encode_expression(encoder, value);
            });
        }
        hir::ExprKind::PointerStructValue(pointer) => {
            encoder.variant(b"pointer-struct-value", |encoder| {
                encode_expression(encoder, pointer);
            });
        }
        hir::ExprKind::MethodReceiver { receiver, plan } => {
            encoder.variant(b"method-receiver", |encoder| {
                encoder.field(b"receiver", |encoder| encode_expression(encoder, receiver));
                encoder.field(b"plan", |encoder| {
                    encode_method_receiver_plan(encoder, plan)
                });
            });
        }
        hir::ExprKind::InterfaceValue {
            value,
            type_identity,
        } => encoder.variant(b"interface-value", |encoder| {
            encoder.field(b"value", |encoder| encode_expression(encoder, value));
            encoder.field(b"type-identity", |encoder| encoder.blob(type_identity));
        }),
        hir::ExprKind::InterfaceCall {
            receiver,
            args,
            candidates,
        } => encoder.variant(b"interface-call", |encoder| {
            encoder.field(b"receiver", |encoder| {
                encode_expression(encoder, receiver);
            });
            encoder.field(b"arguments", |encoder| {
                encoder.sequence(args, encode_expression);
            });
            encoder.field(b"candidates", |encoder| {
                encoder.sequence(candidates, |encoder, candidate| {
                    encoder.field(b"type-identity", |encoder| {
                        encoder.blob(&candidate.type_identity);
                    });
                    encoder.field(b"dynamic-type", |encoder| {
                        ty(encoder, &candidate.dynamic_ty);
                    });
                    encoder.field(b"receiver-plan", |encoder| {
                        encode_method_receiver_plan(encoder, &candidate.receiver_plan);
                    });
                    encoder.field(b"function", |encoder| {
                        qualified_def_id(encoder, candidate.function);
                    });
                });
            });
        }),
        hir::ExprKind::GlobalConstant(id, value) => {
            encoder.variant(b"global-constant", |encoder| {
                encoder.field(b"id", |encoder| qualified_def_id(encoder, *id));
                encoder.field(b"value", |encoder| const_value(encoder, value));
            });
        }
        hir::ExprKind::GlobalVariable(id, value) => {
            encoder.variant(b"global-variable", |encoder| {
                encoder.field(b"id", |encoder| qualified_def_id(encoder, *id));
                encoder.field(b"value", |encoder| encode_static_value(encoder, value));
            });
        }
        hir::ExprKind::Binary { op, left, right } => {
            encoder.variant(b"binary", |encoder| {
                encoder.field(b"operation", |encoder| encode_binary_op(encoder, *op));
                encoder.field(b"left", |encoder| {
                    encode_expression(encoder, left);
                });
                encoder.field(b"right", |encoder| {
                    encode_expression(encoder, right);
                });
            });
        }
        hir::ExprKind::Unary { op, operand } => encoder.variant(b"unary", |encoder| {
            encoder.field(b"operation", |encoder| encode_unary_op(encoder, *op));
            encoder.field(b"operand", |encoder| {
                encode_expression(encoder, operand);
            });
        }),
        hir::ExprKind::Conversion { value } => encoder.variant(b"conversion", |encoder| {
            encode_expression(encoder, value);
        }),
        hir::ExprKind::Recover => encoder.variant(b"recover", |_| {}),
        hir::ExprKind::SliceLiteralI64(elements) => {
            encoder.variant(b"slice-literal-i64", |encoder| {
                encoder.sequence(elements, |encoder, element| encoder.i64(*element));
            });
        }
        hir::ExprKind::DynamicSliceLiteralI64(elements) => {
            encode_expression_list(encoder, b"dynamic-slice-literal-i64", elements);
        }
        hir::ExprKind::AggregateSliceLiteral {
            elements,
            type_identity,
        } => encoder.variant(b"aggregate-slice-literal", |encoder| {
            encoder.field(b"elements", |encoder| {
                encoder.sequence(elements, encode_expression);
            });
            encoder.field(b"type-identity", |encoder| encoder.blob(type_identity));
        }),
        hir::ExprKind::AggregateSliceIndex {
            slice,
            index,
            type_identity,
        } => encoder.variant(b"aggregate-slice-index", |encoder| {
            encoder.field(b"slice", |encoder| encode_expression(encoder, slice));
            encoder.field(b"index", |encoder| encode_expression(encoder, index));
            encoder.field(b"type-identity", |encoder| encoder.blob(type_identity));
        }),
        hir::ExprKind::Append { slice, arguments } => encoder.variant(b"append", |encoder| {
            encoder.field(b"slice", |encoder| encode_expression(encoder, slice));
            encoder.field(b"arguments", |encoder| match arguments {
                hir::AppendArguments::Elements(elements) => {
                    encoder.variant(b"elements", |encoder| {
                        encoder.sequence(elements, encode_expression);
                    });
                }
                hir::AppendArguments::Spread(value) => {
                    encoder.variant(b"spread", |encoder| encode_expression(encoder, value));
                }
            });
        }),
        hir::ExprKind::SliceLiteralU8(elements) => {
            encoder.variant(b"slice-literal-u8", |encoder| encoder.blob(elements));
        }
        hir::ExprKind::SliceLiteralBool(elements) => {
            encoder.variant(b"slice-literal-bool", |encoder| {
                encoder.sequence(elements, |encoder, element| encoder.bool(*element));
            });
        }
        hir::ExprKind::SliceLiteralGoString(elements) => {
            encode_expression_list(encoder, b"slice-literal-go-string", elements);
        }
        hir::ExprKind::ArrayLiteralI64(elements) => {
            encoder.variant(b"array-literal-i64", |encoder| {
                encoder.sequence(elements, |encoder, element| encoder.i64(*element));
            });
        }
        hir::ExprKind::ArrayLiteral(elements) => {
            encoder.variant(b"array-literal", |encoder| {
                encoder.sequence(elements, |encoder, (index, element)| {
                    encoder.field(b"index", |encoder| encoder.u64(*index));
                    encoder.field(b"value", |encoder| encode_expression(encoder, element));
                });
            });
        }
        hir::ExprKind::ArrayIndexI64 { array, index } => {
            encoder.variant(b"array-index-i64", |encoder| {
                encoder.field(b"array", |encoder| encode_expression(encoder, array));
                encoder.field(b"index", |encoder| encode_expression(encoder, index));
            });
        }
        hir::ExprKind::ArrayIndex { array, index } => {
            encoder.variant(b"array-index", |encoder| {
                encoder.field(b"array", |encoder| encode_expression(encoder, array));
                encoder.field(b"index", |encoder| encode_expression(encoder, index));
            });
        }
        hir::ExprKind::ArrayLen { array, length } => {
            encoder.variant(b"array-len", |encoder| {
                encoder.field(b"array", |encoder| encode_expression(encoder, array));
                encoder.field(b"length", |encoder| encoder.u64(*length));
            });
        }
        hir::ExprKind::StructLiteral(fields) => {
            encode_expression_list(encoder, b"struct-literal", fields);
        }
        hir::ExprKind::StructField { structure, field } => {
            encoder.variant(b"struct-field", |encoder| {
                encoder.field(b"structure", |encoder| {
                    encode_expression(encoder, structure);
                });
                encoder.field(b"field", |encoder| encoder.u32(*field));
            });
        }
        hir::ExprKind::MapLiteralStringI64(entries) => {
            encoder.variant(b"map-literal-string-i64", |encoder| {
                encoder.sequence(entries, |encoder, (key, value)| {
                    encoder.field(b"key", |encoder| encode_expression(encoder, key));
                    encoder.field(b"value", |encoder| encode_expression(encoder, value));
                });
            });
        }
        hir::ExprKind::MapLiteralI64GoString(entries) => {
            encoder.variant(b"map-literal-i64-go-string", |encoder| {
                encoder.sequence(entries, |encoder, (key, value)| {
                    encoder.field(b"key", |encoder| encode_expression(encoder, key));
                    encoder.field(b"value", |encoder| encode_expression(encoder, value));
                });
            });
        }
        hir::ExprKind::AggregateMapLiteral {
            entries,
            type_identity,
        } => encoder.variant(b"aggregate-map-literal", |encoder| {
            encoder.field(b"entries", |encoder| {
                encoder.sequence(entries, |encoder, (key, value)| {
                    encoder.field(b"key", |encoder| encode_expression(encoder, key));
                    encoder.field(b"value", |encoder| encode_expression(encoder, value));
                });
            });
            encoder.field(b"type-identity", |encoder| encoder.blob(type_identity));
        }),
        hir::ExprKind::AggregateMapIndex {
            map,
            key,
            type_identity,
        } => encoder.variant(b"aggregate-map-index", |encoder| {
            encoder.field(b"map", |encoder| encode_expression(encoder, map));
            encoder.field(b"key", |encoder| encode_expression(encoder, key));
            encoder.field(b"type-identity", |encoder| encoder.blob(type_identity));
        }),
        hir::ExprKind::Call { callee, args } => encoder.variant(b"call", |encoder| {
            encoder.field(b"callee", |encoder| encode_callee(encoder, *callee));
            encoder.field(b"arguments", |encoder| {
                encoder.sequence(args, |encoder, expression| {
                    encode_expression(encoder, expression);
                });
            });
        }),
        hir::ExprKind::ForwardedCall {
            callee,
            prefix,
            source_call,
            coercions,
            fixed_results,
            variadic_slice,
        } => encoder.variant(b"forwarded-call", |encoder| {
            encoder.field(b"callee", |encoder| encode_callee(encoder, *callee));
            encoder.field(b"prefix", |encoder| {
                encoder.sequence(prefix, encode_expression);
            });
            encoder.field(b"source-call", |encoder| {
                encode_expression(encoder, source_call);
            });
            encoder.field(b"coercions", |encoder| {
                encoder.sequence(coercions, encode_value_coercion);
            });
            encoder.field(b"fixed-results", |encoder| encoder.u32(*fixed_results));
            encoder.field(b"variadic-slice", |encoder| match variadic_slice {
                Some(variadic_slice) => {
                    encoder.variant(b"some", |encoder| ty(encoder, variadic_slice));
                }
                None => encoder.variant(b"none", |_| {}),
            });
        }),
    }
}

fn encode_method_receiver_plan(encoder: &mut Encoder, plan: &hir::MethodReceiverPlan) {
    encoder.field(b"root-type", |encoder| ty(encoder, &plan.root_ty));
    encoder.field(b"path", |encoder| {
        encoder.sequence(&plan.path, |encoder, step| {
            encoder.field(b"owner-type", |encoder| ty(encoder, &step.owner_ty));
            encoder.field(b"field", |encoder| encoder.u32(step.field));
            encoder.field(b"field-type", |encoder| ty(encoder, &step.field_ty));
        });
    });
    encoder.field(b"selected-type", |encoder| ty(encoder, &plan.selected_ty));
    encoder.field(b"adjustment", |encoder| {
        encoder.u8(match plan.adjustment {
            hir::MethodReceiverAdjustment::Identity => 0,
            hir::MethodReceiverAdjustment::AutoAddress => 1,
            hir::MethodReceiverAdjustment::AutoIndirect => 2,
        });
    });
    encoder.field(b"receiver-type", |encoder| ty(encoder, &plan.receiver_ty));
}

fn encode_expression_list(encoder: &mut Encoder, label: &'static [u8], values: &[hir::Expr]) {
    encoder.variant(label, |encoder| encoder.sequence(values, encode_expression));
}

fn encode_callee(encoder: &mut Encoder, callee: hir::Callee) {
    match callee {
        hir::Callee::Function(id) => {
            encoder.variant(b"function", |encoder| qualified_def_id(encoder, id));
        }
        hir::Callee::Closure(id) => {
            encoder.variant(b"closure", |encoder| closure_id(encoder, id));
        }
        hir::Callee::Builtin(builtin) => {
            encoder.variant(b"builtin", |encoder| encode_builtin(encoder, builtin));
        }
    }
}

fn encode_binary_op(encoder: &mut Encoder, op: hir::BinaryOp) {
    encoder.variant(
        match op {
            hir::BinaryOp::Add => b"add",
            hir::BinaryOp::Sub => b"subtract",
            hir::BinaryOp::Mul => b"multiply",
            hir::BinaryOp::Div => b"divide",
            hir::BinaryOp::Rem => b"remainder",
            hir::BinaryOp::BitAnd => b"bit-and",
            hir::BinaryOp::BitOr => b"bit-or",
            hir::BinaryOp::BitXor => b"bit-xor",
            hir::BinaryOp::Shl => b"shift-left",
            hir::BinaryOp::Shr => b"shift-right",
            hir::BinaryOp::AndNot => b"and-not",
            hir::BinaryOp::Equal => b"equal",
            hir::BinaryOp::NotEqual => b"not-equal",
            hir::BinaryOp::Less => b"less",
            hir::BinaryOp::LessEqual => b"less-equal",
            hir::BinaryOp::Greater => b"greater",
            hir::BinaryOp::GreaterEqual => b"greater-equal",
            hir::BinaryOp::LogicalAnd => b"logical-and",
            hir::BinaryOp::LogicalOr => b"logical-or",
            hir::BinaryOp::Min => b"minimum",
            hir::BinaryOp::Max => b"maximum",
            hir::BinaryOp::Complex => b"complex",
        },
        |_| {},
    );
}

fn encode_unary_op(encoder: &mut Encoder, op: hir::UnaryOp) {
    encoder.variant(
        match op {
            hir::UnaryOp::Positive => b"positive",
            hir::UnaryOp::Negative => b"negative",
            hir::UnaryOp::Not => b"not",
            hir::UnaryOp::BitNot => b"bit-not",
            hir::UnaryOp::Real => b"real",
            hir::UnaryOp::Imag => b"imaginary",
        },
        |_| {},
    );
}

fn encode_value_category(encoder: &mut Encoder, category: hir::ValueCategory) {
    encoder.variant(
        match category {
            hir::ValueCategory::Value => b"value",
            hir::ValueCategory::Place => b"place",
            hir::ValueCategory::Constant => b"constant",
        },
        |_| {},
    );
}
