//! Structural, semantic-effect, provenance, and call-ABI MIR verification.

mod arrays;
mod channels;
mod containers;
mod effects;
mod interfaces;
mod pointers;
mod provenance;
mod structs;

use std::collections::{BTreeMap, BTreeSet};

use super::{
    File, Function, LocalDecl, Operand, PanicEdge, Place, Rvalue, RvalueKind, Statement,
    Terminator, TerminatorKind,
};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{BasicBlockId, LocalId, QualifiedDefId};
use crate::compiler::types::{ComplexTy, ConstValue, FloatTy, IntTy, Signature, Ty};
use arrays::{
    verify_array_index, verify_array_literal, verify_array_set, verify_scalar_array_literal,
};
use channels::{is_channel_builtin, verify_channel_call};
use containers::{
    map_string_i64_ty, verify_bool_slice_call, verify_byte_slice_call_arguments,
    verify_map_call_arguments, verify_slice_call_arguments,
};
use effects::{read_effects, verify_effects, verify_panic_edge};
use interfaces::{is_interface_builtin, verify_interface_call};
use pointers::{is_pointer_builtin, verify_pointer_call};
use provenance::{
    verify_rvalue_provenance, verify_source_provenance, verify_source_ref,
    verify_statement_provenance, verify_terminator_provenance,
};
use structs::{verify_struct_field, verify_struct_literal, verify_struct_set};

impl File {
    #[cfg(test)]
    pub(super) fn verify(&self) -> Result<(), Diagnostic> {
        let mut signatures = BTreeMap::new();
        for function in &self.functions {
            if signatures
                .insert(
                    QualifiedDefId::new(self.package_id, function.id),
                    function.signature.clone(),
                )
                .is_some()
            {
                return Err(Diagnostic::backend(format!(
                    "duplicate MIR function DefId {}",
                    function.id
                )));
            }
        }
        self.verify_with_signatures(&signatures)
    }

    pub(super) fn verify_with_signatures(
        &self,
        signatures: &BTreeMap<QualifiedDefId, Signature>,
    ) -> Result<(), Diagnostic> {
        let mut definitions = BTreeSet::new();
        for function in &self.functions {
            if !definitions.insert(function.id) {
                return Err(Diagnostic::backend(format!(
                    "duplicate MIR function DefId {}",
                    function.id
                )));
            }
            let owner = QualifiedDefId::new(self.package_id, function.id);
            match signatures.get(&owner) {
                Some(signature) if signature == &function.signature => {}
                Some(_) => {
                    return Err(Diagnostic::backend(format!(
                        "MIR function DefId {} disagrees with the package signature index",
                        function.id
                    )));
                }
                None => {
                    return Err(Diagnostic::backend(format!(
                        "MIR function DefId {} is absent from the package signature index",
                        function.id
                    )));
                }
            }
            function.verify(signatures)?;
        }
        Ok(())
    }
}

impl Function {
    pub(super) fn verify_with_signatures(
        &self,
        owner: QualifiedDefId,
        signatures: &BTreeMap<QualifiedDefId, Signature>,
    ) -> Result<(), Diagnostic> {
        if owner.definition() != self.id {
            return Err(Diagnostic::backend(format!(
                "MIR owner {owner:?} does not identify function DefId {}",
                self.id
            )));
        }
        match signatures.get(&owner) {
            Some(signature) if signature == &self.signature => {}
            Some(_) => {
                return Err(Diagnostic::backend(format!(
                    "MIR function DefId {} disagrees with the package signature index",
                    self.id
                )));
            }
            None => {
                return Err(Diagnostic::backend(format!(
                    "MIR function DefId {} is absent from the package signature index",
                    self.id
                )));
            }
        }
        self.verify(signatures)
    }

    fn verify(&self, signatures: &BTreeMap<QualifiedDefId, Signature>) -> Result<(), Diagnostic> {
        verify_source_ref(self.source, self.id, "function")?;
        for ty in self.signature.params.iter().chain(&self.signature.results) {
            verify_bootstrap_type(ty, "function signature")?;
        }
        if self.entry.0 as usize >= self.blocks.len() {
            return Err(Diagnostic::backend("MIR entry block does not exist"));
        }
        for (index, local) in self.locals.iter().enumerate() {
            if local.id.0 as usize != index {
                return Err(Diagnostic::backend(format!(
                    "MIR local IDs are not dense: expected {index}, found {}",
                    local.id.0
                )));
            }
            verify_bootstrap_type(&local.ty, "local")?;
        }
        if self.params.len() != self.signature.params.len() {
            return Err(Diagnostic::backend(format!(
                "MIR function {} has {} parameter locals but expects {}",
                self.name,
                self.params.len(),
                self.signature.params.len()
            )));
        }
        let mut parameters = BTreeSet::new();
        for (position, (parameter, expected)) in
            self.params.iter().zip(&self.signature.params).enumerate()
        {
            let declaration = self.local(*parameter)?;
            if !parameters.insert(*parameter) {
                return Err(Diagnostic::backend(format!(
                    "MIR function {} repeats parameter local {}",
                    self.name, parameter.0
                )));
            }
            if declaration.kind != hir::LocalKind::Parameter {
                return Err(Diagnostic::backend(format!(
                    "MIR parameter {position} uses non-parameter local {}",
                    parameter.0
                )));
            }
            verify_same_type(&declaration.ty, expected, "parameter")?;
        }
        for (index, block) in self.blocks.iter().enumerate() {
            if block.id.0 as usize != index {
                return Err(Diagnostic::backend(format!(
                    "MIR block IDs are not dense: expected {index}, found {}",
                    block.id.0
                )));
            }
            verify_source_provenance(&block.provenance, self.id, "basic block")?;
            for statement in &block.statements {
                self.verify_statement(statement)?;
            }
            self.verify_terminator(&block.terminator, signatures)?;
        }
        self.verify_panic_cleanup()?;
        self.verify_definite_initialization()
    }

    fn verify_panic_cleanup(&self) -> Result<(), Diagnostic> {
        if let Some(cleanup) = self.panic_cleanup {
            if cleanup.entry.0 as usize >= self.blocks.len() {
                return Err(Diagnostic::backend(
                    "MIR panic cleanup block does not exist",
                ));
            }
            verify_same_type(
                self.place_ty(Place {
                    local: cleanup.active,
                })?,
                &Ty::Bool,
                "panic cleanup state",
            )?;
        }
        for block in &self.blocks {
            for edge in block
                .statements
                .iter()
                .map(|statement| statement.value.panic)
                .chain(std::iter::once(block.terminator.panic))
            {
                if let PanicEdge::Cleanup(target) = edge
                    && self.panic_cleanup.map(|cleanup| cleanup.entry) != Some(target)
                {
                    return Err(Diagnostic::backend(
                        "MIR panic edge does not target the function cleanup entry",
                    ));
                }
            }
        }
        Ok(())
    }

    fn verify_statement(&self, statement: &Statement) -> Result<(), Diagnostic> {
        verify_statement_provenance(&statement.provenance, self.id)?;
        let destination_ty = self.place_ty(statement.destination)?;
        let value_ty = self.verify_rvalue(&statement.value)?;
        verify_same_type(destination_ty, &value_ty, "assignment")?;
        let expected = statement.value.effects.union(hir::Effects {
            may_write: true,
            ..hir::Effects::default()
        });
        verify_effects(statement.effects, expected, "statement")
    }

    fn verify_rvalue(&self, rvalue: &Rvalue) -> Result<Ty, Diagnostic> {
        verify_rvalue_provenance(&rvalue.provenance, self.id)?;
        let (ty, intrinsic) = match &rvalue.kind {
            RvalueKind::Use(operand) => (self.operand_ty(operand)?, hir::Effects::default()),
            RvalueKind::SliceLiteralI64(_) => (
                Ty::Slice(Box::new(Ty::Int(IntTy::Int))),
                hir::Effects {
                    may_allocate: true,
                    ..hir::Effects::default()
                },
            ),
            RvalueKind::SliceLiteralU8(_) => (
                Ty::Slice(Box::new(Ty::Uint(crate::compiler::types::UintTy::Uint8))),
                hir::Effects {
                    may_allocate: true,
                    ..hir::Effects::default()
                },
            ),
            RvalueKind::SliceLiteralBool(_) => (
                Ty::Slice(Box::new(Ty::Bool)),
                hir::Effects {
                    may_allocate: true,
                    ..hir::Effects::default()
                },
            ),
            RvalueKind::ArrayLiteralI64(values) => verify_array_literal(values)?,
            RvalueKind::ArrayLiteral { elements, ty } => verify_scalar_array_literal(
                elements
                    .iter()
                    .map(|element| self.operand_ty(element))
                    .collect::<Result<Vec<_>, _>>()?,
                ty,
            )?,
            RvalueKind::ArrayIndexI64 { array, index } => {
                verify_array_index(self.operand_ty(array)?, self.operand_ty(index)?)?
            }
            RvalueKind::ArrayIndex { array, index } => {
                verify_array_index(self.operand_ty(array)?, self.operand_ty(index)?)?
            }
            RvalueKind::ArraySetI64 {
                array,
                index,
                value,
            } => verify_array_set(
                self.operand_ty(array)?,
                self.operand_ty(index)?,
                self.operand_ty(value)?,
            )?,
            RvalueKind::ArraySet {
                array,
                index,
                value,
            } => verify_array_set(
                self.operand_ty(array)?,
                self.operand_ty(index)?,
                self.operand_ty(value)?,
            )?,
            RvalueKind::StructLiteral { fields, ty } => verify_struct_literal(
                fields
                    .iter()
                    .map(|field| self.operand_ty(field))
                    .collect::<Result<Vec<_>, _>>()?,
                ty,
            )?,
            RvalueKind::StructField { structure, field } => {
                verify_struct_field(self.operand_ty(structure)?, *field)?
            }
            RvalueKind::StructSet {
                structure,
                field,
                value,
            } => verify_struct_set(self.operand_ty(structure)?, *field, self.operand_ty(value)?)?,
            RvalueKind::Unary { op, operand, ty } => {
                let operand_ty = self.operand_ty(operand)?;
                let valid = match op {
                    hir::UnaryOp::Positive | hir::UnaryOp::Negative => {
                        operand_ty == *ty
                            && matches!(
                                operand_ty.underlying(),
                                Ty::Int(IntTy::Int)
                                    | Ty::Float(FloatTy::Float64)
                                    | Ty::Complex(ComplexTy::Complex128)
                            )
                    }
                    hir::UnaryOp::Not => operand_ty == Ty::Bool && *ty == Ty::Bool,
                    hir::UnaryOp::BitNot => {
                        operand_ty.underlying() == &Ty::Int(IntTy::Int) && operand_ty == *ty
                    }
                    hir::UnaryOp::Real | hir::UnaryOp::Imag => {
                        operand_ty.underlying() == &Ty::Complex(ComplexTy::Complex128)
                            && *ty == Ty::Float(FloatTy::Float64)
                    }
                };
                if !valid {
                    return Err(Diagnostic::backend(format!(
                        "invalid MIR unary operation {op:?}: {operand_ty:?} -> {ty:?}"
                    )));
                }
                (ty.clone(), hir::Effects::default())
            }
            RvalueKind::Conversion { operand, from, ty } => {
                let operand_ty = self.operand_ty(operand)?;
                verify_same_type(&operand_ty, from, "conversion operand")?;
                if !same_mir_representation(from, ty) {
                    return Err(Diagnostic::backend(format!(
                        "MIR conversion changes representation from {from:?} to {ty:?}"
                    )));
                }
                (ty.clone(), hir::Effects::default())
            }
            RvalueKind::RecoverCompareNil { state, .. } => {
                verify_same_type(self.place_ty(*state)?, &Ty::Bool, "panic recovery state")?;
                (
                    Ty::Bool,
                    hir::Effects {
                        may_read: true,
                        may_write: true,
                        ..hir::Effects::default()
                    },
                )
            }
            RvalueKind::Binary {
                op,
                left,
                right,
                ty,
            } => {
                let left_ty = self.operand_ty(left)?;
                let right_ty = self.operand_ty(right)?;
                verify_binary_types(*op, &left_ty, &right_ty, ty)?;
                (ty.clone(), binary_effects(*op, ty))
            }
        };
        let expected = intrinsic.union(read_effects(rvalue_operands(&rvalue.kind)));
        verify_effects(rvalue.effects, expected, "rvalue")?;
        verify_panic_edge(rvalue.effects, rvalue.panic, "rvalue")?;
        Ok(ty)
    }

    fn verify_terminator(
        &self,
        terminator: &Terminator,
        signatures: &BTreeMap<QualifiedDefId, Signature>,
    ) -> Result<(), Diagnostic> {
        verify_terminator_provenance(&terminator.provenance, self.id)?;
        let intrinsic = match &terminator.kind {
            TerminatorKind::Goto(target) => {
                self.verify_target(*target)?;
                hir::Effects::default()
            }
            TerminatorKind::SwitchBool {
                condition,
                then_target,
                else_target,
            } => {
                verify_same_type(&self.operand_ty(condition)?, &Ty::Bool, "switch condition")?;
                self.verify_target(*then_target)?;
                self.verify_target(*else_target)?;
                hir::Effects::default()
            }
            TerminatorKind::Call {
                callee,
                args,
                destinations,
                target,
            } => {
                self.verify_target(*target)?;
                let argument_types = args
                    .iter()
                    .map(|argument| self.operand_ty(argument))
                    .collect::<Result<Vec<_>, _>>()?;
                match callee {
                    hir::Callee::Function(id) => {
                        let signature = signatures.get(id).ok_or_else(|| {
                            Diagnostic::backend(format!("unknown MIR callee DefId {id}"))
                        })?;
                        if argument_types.len() != signature.params.len() {
                            return Err(Diagnostic::backend(format!(
                                "MIR call to DefId {} has {} arguments but expects {}",
                                id,
                                argument_types.len(),
                                signature.params.len()
                            )));
                        }
                        for (actual, expected) in argument_types.iter().zip(&signature.params) {
                            verify_same_type(actual, expected, "call argument")?;
                        }
                        self.verify_call_destinations(destinations, &signature.results)?;
                    }
                    hir::Callee::Closure(id) => {
                        return Err(Diagnostic::backend(format!(
                            "local function {} survived MIR inlining",
                            id.0
                        )));
                    }
                    hir::Callee::Builtin(builtin) => {
                        let results = if is_channel_builtin(*builtin) {
                            let destination_types = destinations
                                .iter()
                                .map(|destination| self.place_ty(*destination).cloned())
                                .collect::<Result<Vec<_>, _>>()?;
                            verify_channel_call(*builtin, &argument_types, &destination_types)?
                        } else if is_pointer_builtin(*builtin) {
                            let destination_types = destinations
                                .iter()
                                .map(|destination| self.place_ty(*destination).cloned())
                                .collect::<Result<Vec<_>, _>>()?;
                            verify_pointer_call(*builtin, &argument_types, &destination_types)?
                        } else if is_interface_builtin(*builtin) {
                            let destination_types = destinations
                                .iter()
                                .map(|destination| self.place_ty(*destination).cloned())
                                .collect::<Result<Vec<_>, _>>()?;
                            verify_interface_call(*builtin, &argument_types, &destination_types)?
                        } else if matches!(
                            builtin,
                            hir::Builtin::SliceBoolIndex | hir::Builtin::SliceBoolSet
                        ) {
                            verify_bool_slice_call(*builtin, &argument_types)?
                        } else {
                            match builtin {
                                hir::Builtin::Print | hir::Builtin::Println => {
                                    for ty in &argument_types {
                                        if !matches!(
                                            ty,
                                            Ty::Bool | Ty::Int(IntTy::Int) | Ty::String
                                        ) {
                                            return Err(Diagnostic::backend(format!(
                                                "print builtin cannot consume MIR operand type {ty:?}"
                                            )));
                                        }
                                    }
                                    Vec::new()
                                }
                                hir::Builtin::Panic => {
                                    if argument_types.len() != 1 {
                                        return Err(Diagnostic::backend(format!(
                                            "panic builtin has {} MIR operands; expected 1",
                                            argument_types.len()
                                        )));
                                    }
                                    if !matches!(
                                        argument_types.as_slice(),
                                        [Ty::Bool | Ty::Int(IntTy::Int) | Ty::String]
                                    ) {
                                        return Err(Diagnostic::backend(
                                            "panic builtin received an unsupported MIR operand type",
                                        ));
                                    }
                                    Vec::new()
                                }
                                hir::Builtin::SliceI64Index => {
                                    verify_slice_call_arguments(&argument_types, 2, "slice index")?;
                                    vec![Ty::Int(IntTy::Int)]
                                }
                                hir::Builtin::SliceI64Range => {
                                    verify_slice_call_arguments(
                                        &argument_types,
                                        4,
                                        "slice expression",
                                    )?;
                                    vec![Ty::Slice(Box::new(Ty::Int(IntTy::Int)))]
                                }
                                hir::Builtin::SliceI64Set => {
                                    verify_slice_call_arguments(
                                        &argument_types,
                                        3,
                                        "slice assignment",
                                    )?;
                                    Vec::new()
                                }
                                hir::Builtin::SliceI64Make => {
                                    if argument_types != [Ty::Int(IntTy::Int), Ty::Int(IntTy::Int)]
                                    {
                                        return Err(Diagnostic::backend(format!(
                                            "invalid MIR slice make argument types: {argument_types:?}"
                                        )));
                                    }
                                    vec![Ty::Slice(Box::new(Ty::Int(IntTy::Int)))]
                                }
                                hir::Builtin::SliceI64Len | hir::Builtin::SliceI64Cap => {
                                    if argument_types != [Ty::Slice(Box::new(Ty::Int(IntTy::Int)))]
                                    {
                                        return Err(Diagnostic::backend(format!(
                                            "invalid MIR slice len/cap argument types: {argument_types:?}"
                                        )));
                                    }
                                    vec![Ty::Int(IntTy::Int)]
                                }
                                hir::Builtin::SliceI64Append => {
                                    verify_slice_call_arguments(
                                        &argument_types,
                                        2,
                                        "slice append",
                                    )?;
                                    vec![Ty::Slice(Box::new(Ty::Int(IntTy::Int)))]
                                }
                                hir::Builtin::SliceU8AppendSlice => {
                                    verify_byte_slice_call_arguments(
                                        &argument_types,
                                        &Ty::Slice(Box::new(Ty::Uint(
                                            crate::compiler::types::UintTy::Uint8,
                                        ))),
                                        "byte slice append",
                                    )?;
                                    vec![Ty::Slice(Box::new(Ty::Uint(
                                        crate::compiler::types::UintTy::Uint8,
                                    )))]
                                }
                                hir::Builtin::SliceU8AppendString
                                | hir::Builtin::SliceU8CopyString => {
                                    verify_byte_slice_call_arguments(
                                        &argument_types,
                                        &Ty::String,
                                        "string to byte slice operation",
                                    )?;
                                    if *builtin == hir::Builtin::SliceU8CopyString {
                                        vec![Ty::Int(IntTy::Int)]
                                    } else {
                                        vec![Ty::Slice(Box::new(Ty::Uint(
                                            crate::compiler::types::UintTy::Uint8,
                                        )))]
                                    }
                                }
                                hir::Builtin::SliceI64Copy => {
                                    if argument_types
                                        != [
                                            Ty::Slice(Box::new(Ty::Int(IntTy::Int))),
                                            Ty::Slice(Box::new(Ty::Int(IntTy::Int))),
                                        ]
                                    {
                                        return Err(Diagnostic::backend(format!(
                                            "invalid MIR slice copy arguments: {argument_types:?}"
                                        )));
                                    }
                                    vec![Ty::Int(IntTy::Int)]
                                }
                                hir::Builtin::SliceI64Clear => {
                                    if argument_types != [Ty::Slice(Box::new(Ty::Int(IntTy::Int)))]
                                    {
                                        return Err(Diagnostic::backend(format!(
                                            "invalid MIR slice clear arguments: {argument_types:?}"
                                        )));
                                    }
                                    Vec::new()
                                }
                                hir::Builtin::StringFromSliceU8 => {
                                    if argument_types
                                        != [Ty::Slice(Box::new(Ty::Uint(
                                            crate::compiler::types::UintTy::Uint8,
                                        )))]
                                    {
                                        return Err(Diagnostic::backend(format!(
                                            "invalid MIR byte slice conversion arguments: {argument_types:?}"
                                        )));
                                    }
                                    vec![Ty::String]
                                }
                                hir::Builtin::StringLen => {
                                    if argument_types != [Ty::String] {
                                        return Err(Diagnostic::backend(format!(
                                            "invalid MIR string len arguments: {argument_types:?}"
                                        )));
                                    }
                                    vec![Ty::Int(IntTy::Int)]
                                }
                                hir::Builtin::MapStringI64Nil | hir::Builtin::MapStringI64Make => {
                                    verify_map_call_arguments(
                                        &argument_types,
                                        &[],
                                        "map creation",
                                    )?;
                                    vec![map_string_i64_ty()]
                                }
                                hir::Builtin::MapStringI64Len => {
                                    verify_map_call_arguments(
                                        &argument_types,
                                        &[map_string_i64_ty()],
                                        "map len",
                                    )?;
                                    vec![Ty::Int(IntTy::Int)]
                                }
                                hir::Builtin::MapStringI64Get => {
                                    verify_map_call_arguments(
                                        &argument_types,
                                        &[map_string_i64_ty(), Ty::String],
                                        "map lookup",
                                    )?;
                                    vec![Ty::Int(IntTy::Int)]
                                }
                                hir::Builtin::MapStringI64Lookup => {
                                    return Err(Diagnostic::backend(
                                        "map comma-ok lookup survived MIR construction",
                                    ));
                                }
                                hir::Builtin::MapStringI64Contains => {
                                    verify_map_call_arguments(
                                        &argument_types,
                                        &[map_string_i64_ty(), Ty::String],
                                        "map membership test",
                                    )?;
                                    vec![Ty::Bool]
                                }
                                hir::Builtin::MapStringI64Set => {
                                    verify_map_call_arguments(
                                        &argument_types,
                                        &[map_string_i64_ty(), Ty::String, Ty::Int(IntTy::Int)],
                                        "map assignment",
                                    )?;
                                    Vec::new()
                                }
                                hir::Builtin::MapStringI64Delete => {
                                    verify_map_call_arguments(
                                        &argument_types,
                                        &[map_string_i64_ty(), Ty::String],
                                        "map delete",
                                    )?;
                                    Vec::new()
                                }
                                hir::Builtin::MapStringI64Clear => {
                                    verify_map_call_arguments(
                                        &argument_types,
                                        &[map_string_i64_ty()],
                                        "map clear",
                                    )?;
                                    Vec::new()
                                }
                                hir::Builtin::MapStringI64IsNil => {
                                    verify_map_call_arguments(
                                        &argument_types,
                                        &[map_string_i64_ty()],
                                        "map nil comparison",
                                    )?;
                                    vec![Ty::Bool]
                                }
                                hir::Builtin::MapStringI64KeyAt => {
                                    verify_map_call_arguments(
                                        &argument_types,
                                        &[map_string_i64_ty(), Ty::Int(IntTy::Int)],
                                        "map range key",
                                    )?;
                                    vec![Ty::String]
                                }
                                hir::Builtin::ChannelI64Nil
                                | hir::Builtin::ChannelI64Make
                                | hir::Builtin::ChannelI64Len
                                | hir::Builtin::ChannelI64Cap
                                | hir::Builtin::ChannelI64Send
                                | hir::Builtin::ChannelI64ReceiveValue
                                | hir::Builtin::ChannelI64Receive
                                | hir::Builtin::ChannelI64Close
                                | hir::Builtin::ChannelI64IsNil
                                | hir::Builtin::ChannelI64TrySend
                                | hir::Builtin::ChannelI64TryReceive => {
                                    return Err(Diagnostic::backend(
                                        "channel builtin bypassed dedicated MIR verification",
                                    ));
                                }
                                hir::Builtin::PointerI64Nil
                                | hir::Builtin::PointerI64New
                                | hir::Builtin::PointerI64Get
                                | hir::Builtin::PointerI64Set
                                | hir::Builtin::PointerI64IsNil
                                | hir::Builtin::PointerStructI64Nil
                                | hir::Builtin::PointerStructI64New
                                | hir::Builtin::PointerStructI64Get
                                | hir::Builtin::PointerStructI64Set
                                | hir::Builtin::PointerStructI64IsNil
                                | hir::Builtin::PointerStructI64Equal => {
                                    return Err(Diagnostic::backend(
                                        "pointer builtin bypassed dedicated MIR verification",
                                    ));
                                }
                                hir::Builtin::InterfaceNil
                                | hir::Builtin::InterfaceBoxBool
                                | hir::Builtin::InterfaceBoxI64
                                | hir::Builtin::InterfaceBoxGoString
                                | hir::Builtin::InterfaceBoxStructI64
                                | hir::Builtin::InterfaceBoxPointerStructI64
                                | hir::Builtin::InterfaceIsNil
                                | hir::Builtin::InterfaceIsType
                                | hir::Builtin::InterfaceAssert
                                | hir::Builtin::InterfaceUnboxBool
                                | hir::Builtin::InterfaceUnboxI64
                                | hir::Builtin::InterfaceUnboxGoString
                                | hir::Builtin::InterfaceStructI64Get
                                | hir::Builtin::InterfaceUnboxPointerStructI64 => {
                                    return Err(Diagnostic::backend(
                                        "interface builtin bypassed dedicated MIR verification",
                                    ));
                                }
                                hir::Builtin::SliceBoolIndex | hir::Builtin::SliceBoolSet => {
                                    return Err(Diagnostic::backend(
                                        "bool slice builtin bypassed dedicated MIR verification",
                                    ));
                                }
                            }
                        };
                        self.verify_call_destinations(destinations, &results)?;
                    }
                }
                call_effects()
            }
            TerminatorKind::Return(values) => {
                if values.len() != self.signature.results.len() {
                    return Err(Diagnostic::backend(format!(
                        "MIR return has {} values but signature has {}",
                        values.len(),
                        self.signature.results.len()
                    )));
                }
                for (value, expected) in values.iter().zip(&self.signature.results) {
                    verify_same_type(&self.operand_ty(value)?, expected, "return value")?;
                }
                hir::Effects::default()
            }
            TerminatorKind::Unreachable => hir::Effects::default(),
        };
        let expected = intrinsic.union(read_effects(terminator_operands(&terminator.kind)));
        verify_effects(terminator.effects, expected, "terminator")?;
        verify_panic_edge(terminator.effects, terminator.panic, "terminator")
    }

    fn verify_call_destinations(
        &self,
        destinations: &[Place],
        results: &[Ty],
    ) -> Result<(), Diagnostic> {
        if destinations.len() != results.len() {
            return Err(Diagnostic::backend(format!(
                "MIR call has {} destinations for {} result values",
                destinations.len(),
                results.len()
            )));
        }
        for (destination, result) in destinations.iter().zip(results) {
            verify_same_type(self.place_ty(*destination)?, result, "call destination")?;
        }
        Ok(())
    }

    fn verify_target(&self, target: BasicBlockId) -> Result<(), Diagnostic> {
        ((target.0 as usize) < self.blocks.len())
            .then_some(())
            .ok_or_else(|| Diagnostic::backend(format!("invalid MIR target {}", target.0)))
    }

    fn local(&self, local: LocalId) -> Result<&LocalDecl, Diagnostic> {
        self.locals
            .get(local.0 as usize)
            .ok_or_else(|| Diagnostic::backend(format!("invalid MIR local {}", local.0)))
    }

    fn place_ty(&self, place: Place) -> Result<&Ty, Diagnostic> {
        self.local(place.local).map(|local| &local.ty)
    }

    fn operand_ty(&self, operand: &Operand) -> Result<Ty, Diagnostic> {
        match operand {
            Operand::Read(place) => Ok(self.place_ty(*place)?.clone()),
            Operand::Constant(value, ty) => {
                verify_bootstrap_type(ty, "constant operand")?;
                verify_constant_type(value, ty)?;
                Ok(ty.clone())
            }
            Operand::Unit => Ok(Ty::Unit),
        }
    }
}

fn rvalue_operands(kind: &RvalueKind) -> Vec<&Operand> {
    match kind {
        RvalueKind::Use(operand)
        | RvalueKind::Unary { operand, .. }
        | RvalueKind::Conversion { operand, .. } => vec![operand],
        RvalueKind::Binary { left, right, .. } => vec![left, right],
        RvalueKind::ArrayIndexI64 { array, index } => vec![array, index],
        RvalueKind::ArrayIndex { array, index } => vec![array, index],
        RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        } => vec![array, index, value],
        RvalueKind::ArraySet {
            array,
            index,
            value,
        } => vec![array, index, value],
        RvalueKind::ArrayLiteral { elements, .. } => elements.iter().collect(),
        RvalueKind::StructLiteral { fields, .. } => fields.iter().collect(),
        RvalueKind::StructField { structure, .. } => vec![structure],
        RvalueKind::StructSet {
            structure, value, ..
        } => vec![structure, value],
        RvalueKind::RecoverCompareNil { .. } => Vec::new(),
        RvalueKind::SliceLiteralI64(_)
        | RvalueKind::SliceLiteralU8(_)
        | RvalueKind::SliceLiteralBool(_)
        | RvalueKind::ArrayLiteralI64(_) => Vec::new(),
    }
}

fn terminator_operands(kind: &TerminatorKind) -> Vec<&Operand> {
    match kind {
        TerminatorKind::SwitchBool { condition, .. } => vec![condition],
        TerminatorKind::Call { args, .. } | TerminatorKind::Return(args) => args.iter().collect(),
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => Vec::new(),
    }
}

fn binary_effects(op: hir::BinaryOp, result: &Ty) -> hir::Effects {
    hir::Effects {
        may_allocate: op == hir::BinaryOp::Add && result == &Ty::String,
        may_panic: matches!(
            op,
            hir::BinaryOp::Div | hir::BinaryOp::Rem | hir::BinaryOp::Shl | hir::BinaryOp::Shr
        ),
        ..hir::Effects::default()
    }
}

fn call_effects() -> hir::Effects {
    hir::Effects {
        may_call: true,
        may_allocate: true,
        may_block: true,
        may_panic: true,
        may_write: true,
        ..hir::Effects::default()
    }
}

fn verify_bootstrap_type(ty: &Ty, context: &str) -> Result<(), Diagnostic> {
    if *ty == Ty::Unit || ty.is_bootstrap_value() {
        Ok(())
    } else {
        Err(Diagnostic::backend(format!(
            "unsupported type reached {context} MIR: {ty:?}"
        )))
    }
}

fn verify_constant_type(value: &ConstValue, ty: &Ty) -> Result<(), Diagnostic> {
    let underlying = ty.underlying();
    matches!(
        (value, underlying),
        (ConstValue::Bool(_), Ty::Bool)
            | (ConstValue::Int(_), Ty::Int(IntTy::Int))
            | (ConstValue::Float(_), Ty::Float(FloatTy::Float64))
            | (ConstValue::Int(_), Ty::Float(FloatTy::Float64))
            | (
                ConstValue::Complex { .. },
                Ty::Complex(ComplexTy::Complex128)
            )
            | (ConstValue::Int(_), Ty::Complex(ComplexTy::Complex128))
            | (ConstValue::Float(_), Ty::Complex(ComplexTy::Complex128))
            | (ConstValue::String(_), Ty::String)
    )
    .then_some(())
    .ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR constant {value:?} does not have declared type {ty:?}"
        ))
    })
}

fn verify_same_type(actual: &Ty, expected: &Ty, context: &str) -> Result<(), Diagnostic> {
    (actual == expected).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR {context} type mismatch: expected {expected:?}, found {actual:?}"
        ))
    })
}

fn same_mir_representation(left: &Ty, right: &Ty) -> bool {
    left.underlying() == right.underlying()
        || matches!(
            (left.underlying(), right.underlying()),
            (Ty::Channel(_, left), Ty::Channel(_, right)) if left == right
        )
}

fn verify_binary_types(
    op: hir::BinaryOp,
    left: &Ty,
    right: &Ty,
    result: &Ty,
) -> Result<(), Diagnostic> {
    let same_operands = left == right;
    let same_result = result == left;
    let underlying = left.underlying();
    let valid = match op {
        hir::BinaryOp::Add => {
            same_operands
                && same_result
                && matches!(
                    underlying,
                    Ty::Int(IntTy::Int)
                        | Ty::Float(FloatTy::Float64)
                        | Ty::Complex(ComplexTy::Complex128)
                        | Ty::String
                )
        }
        hir::BinaryOp::Sub | hir::BinaryOp::Mul | hir::BinaryOp::Div => {
            same_operands
                && same_result
                && matches!(
                    underlying,
                    Ty::Int(IntTy::Int)
                        | Ty::Float(FloatTy::Float64)
                        | Ty::Complex(ComplexTy::Complex128)
                )
        }
        hir::BinaryOp::Min | hir::BinaryOp::Max => {
            same_operands
                && same_result
                && matches!(
                    underlying,
                    Ty::Int(IntTy::Int) | Ty::Float(FloatTy::Float64)
                )
        }
        hir::BinaryOp::Complex => {
            same_operands
                && *underlying == Ty::Float(FloatTy::Float64)
                && result == &Ty::Complex(ComplexTy::Complex128)
        }
        hir::BinaryOp::Rem
        | hir::BinaryOp::BitAnd
        | hir::BinaryOp::BitOr
        | hir::BinaryOp::BitXor
        | hir::BinaryOp::Shl
        | hir::BinaryOp::Shr
        | hir::BinaryOp::AndNot => {
            same_operands && same_result && *underlying == Ty::Int(IntTy::Int)
        }
        hir::BinaryOp::Equal | hir::BinaryOp::NotEqual => {
            same_operands
                && (matches!(
                    underlying,
                    Ty::Bool
                        | Ty::Int(IntTy::Int)
                        | Ty::Float(FloatTy::Float64)
                        | Ty::Complex(ComplexTy::Complex128)
                        | Ty::String
                ) || underlying.is_bootstrap_comparable_aggregate())
                && result == &Ty::Bool
        }
        hir::BinaryOp::Less
        | hir::BinaryOp::LessEqual
        | hir::BinaryOp::Greater
        | hir::BinaryOp::GreaterEqual => {
            same_operands
                && matches!(
                    underlying,
                    Ty::Int(IntTy::Int) | Ty::Float(FloatTy::Float64) | Ty::String
                )
                && result == &Ty::Bool
        }
        hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr => {
            left == &Ty::Bool && right == &Ty::Bool && result == &Ty::Bool
        }
    };
    valid.then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "invalid MIR binary operation {op:?}: {left:?}, {right:?} -> {result:?}"
        ))
    })
}
