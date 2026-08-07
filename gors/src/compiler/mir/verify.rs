//! Structural, semantic-effect, provenance, and call-ABI MIR verification.

mod arrays;
mod channels;
mod containers;
mod effects;
mod interfaces;
mod pointers;
mod printing;
mod provenance;
mod recovery;
mod strings;
mod structs;
mod type_rules;
use std::collections::{BTreeMap, BTreeSet};

use super::construct::spawn_empty_effects;
use super::{
    File, Function, LocalDecl, Operand, Place, Rvalue, RvalueKind, Statement, Terminator,
    TerminatorKind,
};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{BasicBlockId, LocalId, QualifiedDefId};
use crate::compiler::types::{ComplexTy, FloatTy, IntTy, Signature, Ty, UintTy};
use arrays::{
    verify_array_index, verify_array_literal, verify_array_set, verify_scalar_array_literal,
};
use channels::{is_channel_builtin, verify_channel_call};
use containers::{
    is_aggregate_container_builtin, is_go_string_slice_builtin, is_map_builtin,
    is_representation_slice_builtin, verify_aggregate_container_call, verify_bool_slice_call,
    verify_byte_slice_call_arguments, verify_byte_slice_integer_arguments,
    verify_go_string_slice_call, verify_map_call, verify_representation_slice_call,
    verify_slice_call_arguments, verify_slice_value_arguments,
};
use effects::{
    binary_effects, call_effects, read_effects, rvalue_operands, terminator_operands,
    verify_effects, verify_panic_edge,
};
use interfaces::{is_interface_builtin, verify_interface_call};
use pointers::{is_pointer_builtin, verify_pointer_call};
use printing::verify_print_arguments;
use provenance::{
    verify_rvalue_provenance, verify_source_provenance, verify_source_ref,
    verify_statement_provenance, verify_terminator_provenance,
};
use strings::{is_string_builtin, verify_string_call};
use structs::{verify_struct_field, verify_struct_literal, verify_struct_set};
use type_rules::{
    same_mir_representation, verify_binary_types, verify_bootstrap_type, verify_constant_type,
    verify_same_type,
};

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
        recovery::verify_panic_cleanup(self)?;
        self.verify_definite_initialization()
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
            RvalueKind::SliceLiteralI64 { ty, .. } => {
                let Ty::Slice(element) = ty.underlying() else {
                    return Err(Diagnostic::backend(
                        "integer slice literal MIR omitted its slice type",
                    ));
                };
                if !matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32)) {
                    return Err(Diagnostic::backend(format!(
                        "integer slice literal MIR has unsupported type {ty:?}"
                    )));
                }
                (
                    ty.clone(),
                    hir::Effects {
                        may_allocate: true,
                        ..hir::Effects::default()
                    },
                )
            }
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
                                Ty::Int(_)
                                    | Ty::Uint(_)
                                    | Ty::Float(_)
                                    | Ty::Complex(ComplexTy::Complex128)
                            )
                    }
                    hir::UnaryOp::Not => operand_ty == *ty && operand_ty.underlying() == &Ty::Bool,
                    hir::UnaryOp::BitNot => {
                        matches!(operand_ty.underlying(), Ty::Int(_) | Ty::Uint(_))
                            && operand_ty == *ty
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
            RvalueKind::Recover { state, value } => {
                verify_same_type(self.place_ty(*state)?, &Ty::Bool, "panic recovery state")?;
                let recovered_ty = Ty::Interface(Vec::new());
                verify_same_type(
                    &self.operand_ty(value)?,
                    &recovered_ty,
                    "panic recovery value",
                )?;
                (
                    recovered_ty,
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
                (ty.clone(), binary_effects(*op, ty, &right_ty))
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
                        } else if is_representation_slice_builtin(*builtin) {
                            let destination_types = destinations
                                .iter()
                                .map(|destination| self.place_ty(*destination).cloned())
                                .collect::<Result<Vec<_>, _>>()?;
                            verify_representation_slice_call(
                                *builtin,
                                &argument_types,
                                &destination_types,
                            )?
                        } else if is_map_builtin(*builtin) {
                            let destination_types = destinations
                                .iter()
                                .map(|destination| self.place_ty(*destination).cloned())
                                .collect::<Result<Vec<_>, _>>()?;
                            verify_map_call(*builtin, &argument_types, &destination_types)?
                        } else if is_go_string_slice_builtin(*builtin) {
                            let destination_types = destinations
                                .iter()
                                .map(|destination| self.place_ty(*destination).cloned())
                                .collect::<Result<Vec<_>, _>>()?;
                            verify_go_string_slice_call(
                                *builtin,
                                &argument_types,
                                &destination_types,
                            )?
                        } else if is_string_builtin(*builtin) {
                            let destination_types = destinations
                                .iter()
                                .map(|destination| self.place_ty(*destination).cloned())
                                .collect::<Result<Vec<_>, _>>()?;
                            verify_string_call(*builtin, &argument_types, &destination_types)?
                        } else if matches!(
                            builtin,
                            hir::Builtin::SliceBoolIndex | hir::Builtin::SliceBoolSet
                        ) {
                            verify_bool_slice_call(*builtin, &argument_types)?
                        } else if is_aggregate_container_builtin(*builtin) {
                            let destination_types = destinations
                                .iter()
                                .map(|destination| self.place_ty(*destination).cloned())
                                .collect::<Result<Vec<_>, _>>()?;
                            verify_aggregate_container_call(
                                *builtin,
                                &argument_types,
                                &destination_types,
                            )?
                        } else {
                            match builtin {
                                hir::Builtin::Print | hir::Builtin::Println => {
                                    verify_print_arguments(&argument_types)?
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
                                        [Ty::Interface(methods)] if methods.is_empty()
                                    ) {
                                        return Err(Diagnostic::backend(
                                            "panic builtin received an unsupported MIR operand type",
                                        ));
                                    }
                                    Vec::new()
                                }
                                hir::Builtin::SliceI64Index => {
                                    vec![verify_slice_call_arguments(
                                        &argument_types,
                                        2,
                                        "slice index",
                                    )?]
                                }
                                hir::Builtin::SliceI64Range => {
                                    let element = verify_slice_call_arguments(
                                        &argument_types,
                                        4,
                                        "slice expression",
                                    )?;
                                    vec![Ty::Slice(Box::new(element))]
                                }
                                hir::Builtin::SliceI64Set => {
                                    verify_slice_value_arguments(
                                        &argument_types,
                                        true,
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
                                hir::Builtin::SliceU8Make => {
                                    if argument_types != [Ty::Int(IntTy::Int), Ty::Int(IntTy::Int)]
                                    {
                                        return Err(Diagnostic::backend(format!(
                                            "invalid MIR byte slice make argument types: {argument_types:?}"
                                        )));
                                    }
                                    vec![Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8)))]
                                }
                                hir::Builtin::SliceI64Len | hir::Builtin::SliceI64Cap => {
                                    let valid = matches!(
                                        argument_types.as_slice(),
                                        [slice]
                                            if matches!(
                                                slice.underlying(),
                                                Ty::Slice(element)
                                                    if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32))
                                            )
                                    );
                                    if !valid {
                                        return Err(Diagnostic::backend(format!(
                                            "invalid MIR slice len/cap argument types: {argument_types:?}"
                                        )));
                                    }
                                    vec![Ty::Int(IntTy::Int)]
                                }
                                hir::Builtin::SliceI64Append => {
                                    let slice = verify_slice_value_arguments(
                                        &argument_types,
                                        false,
                                        "slice append",
                                    )?;
                                    vec![slice]
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
                                hir::Builtin::SliceU8Set => {
                                    let expected = [
                                        Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8))),
                                        Ty::Int(IntTy::Int),
                                        Ty::Uint(UintTy::Uint8),
                                    ];
                                    if argument_types != expected {
                                        return Err(Diagnostic::backend(format!(
                                            "invalid MIR byte slice set argument types: {argument_types:?}"
                                        )));
                                    }
                                    Vec::new()
                                }
                                hir::Builtin::SliceU8Copy => {
                                    let byte_slice = Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8)));
                                    if argument_types != [byte_slice.clone(), byte_slice] {
                                        return Err(Diagnostic::backend(format!(
                                            "invalid MIR byte slice copy arguments: {argument_types:?}"
                                        )));
                                    }
                                    vec![Ty::Int(IntTy::Int)]
                                }
                                hir::Builtin::SliceU8Len => {
                                    verify_byte_slice_integer_arguments(
                                        &argument_types,
                                        1,
                                        "byte slice len",
                                    )?;
                                    vec![Ty::Int(IntTy::Int)]
                                }
                                hir::Builtin::SliceU8Index => {
                                    verify_byte_slice_integer_arguments(
                                        &argument_types,
                                        2,
                                        "byte slice index",
                                    )?;
                                    vec![Ty::Uint(UintTy::Uint8)]
                                }
                                hir::Builtin::SliceU8Range => {
                                    verify_byte_slice_integer_arguments(
                                        &argument_types,
                                        4,
                                        "byte slice expression",
                                    )?;
                                    vec![Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8)))]
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
                                hir::Builtin::MapStringI64Nil
                                | hir::Builtin::MapStringI64Make
                                | hir::Builtin::MapStringI64Len
                                | hir::Builtin::MapStringI64Get
                                | hir::Builtin::MapStringI64Lookup
                                | hir::Builtin::MapStringI64Contains
                                | hir::Builtin::MapStringI64Set
                                | hir::Builtin::MapStringI64Delete
                                | hir::Builtin::MapStringI64Clear
                                | hir::Builtin::MapStringI64IsNil
                                | hir::Builtin::MapStringI64KeyAt
                                | hir::Builtin::MapStringI64RangeKeys
                                | hir::Builtin::MapI64GoStringNil
                                | hir::Builtin::MapI64GoStringMake
                                | hir::Builtin::MapI64GoStringLen
                                | hir::Builtin::MapI64GoStringGet
                                | hir::Builtin::MapI64GoStringLookup
                                | hir::Builtin::MapI64GoStringContains
                                | hir::Builtin::MapI64GoStringSet
                                | hir::Builtin::MapI64GoStringDelete
                                | hir::Builtin::MapI64GoStringClear
                                | hir::Builtin::MapI64GoStringIsNil
                                | hir::Builtin::MapI64GoStringRangeKeys => {
                                    return Err(Diagnostic::backend(
                                        "map builtin bypassed MIR verifier dispatch",
                                    ));
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
                                | hir::Builtin::ChannelI64TryReceive
                                | hir::Builtin::ChannelGoStringNil
                                | hir::Builtin::ChannelGoStringMake
                                | hir::Builtin::ChannelGoStringLen
                                | hir::Builtin::ChannelGoStringCap
                                | hir::Builtin::ChannelGoStringSend
                                | hir::Builtin::ChannelGoStringReceiveValue
                                | hir::Builtin::ChannelGoStringReceive
                                | hir::Builtin::ChannelGoStringClose
                                | hir::Builtin::ChannelGoStringIsNil
                                | hir::Builtin::ChannelGoStringTrySend
                                | hir::Builtin::ChannelGoStringTryReceive
                                | hir::Builtin::ChannelGoChannelI64Nil
                                | hir::Builtin::ChannelGoChannelI64Make
                                | hir::Builtin::ChannelGoChannelI64Len
                                | hir::Builtin::ChannelGoChannelI64Cap
                                | hir::Builtin::ChannelGoChannelI64Send
                                | hir::Builtin::ChannelGoChannelI64ReceiveValue
                                | hir::Builtin::ChannelGoChannelI64Receive
                                | hir::Builtin::ChannelGoChannelI64Close
                                | hir::Builtin::ChannelGoChannelI64IsNil
                                | hir::Builtin::ChannelGoChannelI64TrySend
                                | hir::Builtin::ChannelGoChannelI64TryReceive => {
                                    return Err(Diagnostic::backend(
                                        "channel builtin bypassed dedicated MIR verification",
                                    ));
                                }
                                hir::Builtin::PointerI64Nil
                                | hir::Builtin::PointerI64New
                                | hir::Builtin::PointerI64Get
                                | hir::Builtin::PointerI64Set
                                | hir::Builtin::PointerI64IsNil
                                | hir::Builtin::PointerI64Equal
                                | hir::Builtin::PointerStructI64Nil
                                | hir::Builtin::PointerStructI64New
                                | hir::Builtin::PointerStructI64Get
                                | hir::Builtin::PointerStructI64Set
                                | hir::Builtin::PointerStructI64IsNil
                                | hir::Builtin::PointerStructI64Equal
                                | hir::Builtin::AggregatePointerNil
                                | hir::Builtin::AggregatePointerNew
                                | hir::Builtin::AggregatePointerSnapshot
                                | hir::Builtin::AggregatePointerIsNil => {
                                    return Err(Diagnostic::backend(
                                        "pointer builtin bypassed dedicated MIR verification",
                                    ));
                                }
                                hir::Builtin::InterfaceNil
                                | hir::Builtin::InterfaceBoxBool
                                | hir::Builtin::InterfaceBoxF64
                                | hir::Builtin::InterfaceBoxI64
                                | hir::Builtin::InterfaceBoxGoString
                                | hir::Builtin::InterfaceBoxStructI64
                                | hir::Builtin::InterfaceBoxPointerI64
                                | hir::Builtin::InterfaceBoxPointerStructI64
                                | hir::Builtin::InterfaceBoxAggregate
                                | hir::Builtin::InterfaceBoxComparableAggregate
                                | hir::Builtin::InterfaceEqual
                                | hir::Builtin::InterfaceIsNil
                                | hir::Builtin::InterfaceIsType
                                | hir::Builtin::InterfaceIsRuntimeError
                                | hir::Builtin::InterfaceAssert
                                | hir::Builtin::InterfaceSatisfies
                                | hir::Builtin::InterfaceSatisfiesRuntimeError
                                | hir::Builtin::InterfaceSatisfiesNonNil
                                | hir::Builtin::InterfaceUnboxBool
                                | hir::Builtin::InterfaceUnboxF64
                                | hir::Builtin::InterfaceUnboxI64
                                | hir::Builtin::InterfaceUnboxGoString
                                | hir::Builtin::InterfaceBoxGoSliceGoString
                                | hir::Builtin::InterfaceUnboxGoSliceGoString
                                | hir::Builtin::InterfaceStructI64Get
                                | hir::Builtin::InterfaceUnboxPointerI64
                                | hir::Builtin::InterfaceUnboxPointerStructI64
                                | hir::Builtin::InterfaceUnboxAggregate
                                | hir::Builtin::FunctionNil
                                | hir::Builtin::FunctionIsNil => {
                                    return Err(Diagnostic::backend(
                                        "interface builtin bypassed dedicated MIR verification",
                                    ));
                                }
                                hir::Builtin::StringFromRune
                                | hir::Builtin::StringFromSliceU8
                                | hir::Builtin::StringFromSliceRunes
                                | hir::Builtin::StringToSliceRunes
                                | hir::Builtin::StringLen
                                | hir::Builtin::StringIndex
                                | hir::Builtin::StringRange
                                | hir::Builtin::StringRangeCount
                                | hir::Builtin::StringRangeIndexAt
                                | hir::Builtin::StringRangeRuneAt => {
                                    return Err(Diagnostic::backend(
                                        "string builtin bypassed dedicated MIR verification",
                                    ));
                                }
                                hir::Builtin::SliceBoolIndex | hir::Builtin::SliceBoolSet => {
                                    return Err(Diagnostic::backend(
                                        "bool slice builtin bypassed dedicated MIR verification",
                                    ));
                                }
                                hir::Builtin::SliceGoStringIndex
                                | hir::Builtin::SliceGoStringRange
                                | hir::Builtin::SliceGoStringSet
                                | hir::Builtin::SliceGoStringMake
                                | hir::Builtin::SliceGoStringNil
                                | hir::Builtin::SliceGoStringIsNil
                                | hir::Builtin::SliceGoStringLen
                                | hir::Builtin::SliceGoStringCap
                                | hir::Builtin::SliceGoStringAppend
                                | hir::Builtin::SliceGoStringCopy
                                | hir::Builtin::SliceGoStringClear => {
                                    return Err(Diagnostic::backend(
                                        "string slice builtin bypassed dedicated MIR verification",
                                    ));
                                }
                                hir::Builtin::SliceI64Nil
                                | hir::Builtin::SliceI64IsNil
                                | hir::Builtin::SliceU8Nil
                                | hir::Builtin::SliceU8IsNil
                                | hir::Builtin::SliceBoolNil
                                | hir::Builtin::SliceBoolIsNil
                                | hir::Builtin::AggregateSliceNil
                                | hir::Builtin::AggregateSliceIsNil
                                | hir::Builtin::SnapshotFunctionSliceAppend
                                | hir::Builtin::SnapshotFunctionSliceCall => {
                                    return Err(Diagnostic::backend(
                                        "slice representation builtin bypassed dedicated MIR verification",
                                    ));
                                }
                                hir::Builtin::AggregateSliceMake
                                | hir::Builtin::AggregateSliceLen
                                | hir::Builtin::AggregateSliceIndexTagged
                                | hir::Builtin::AggregateSliceSetTagged
                                | hir::Builtin::AggregateMapMake
                                | hir::Builtin::AggregateMapLen
                                | hir::Builtin::AggregateMapGetTagged
                                | hir::Builtin::AggregateMapContains
                                | hir::Builtin::AggregateMapSetTagged => {
                                    return Err(Diagnostic::backend(
                                        "aggregate container builtin bypassed dedicated MIR verification",
                                    ));
                                }
                            }
                        };
                        self.verify_call_destinations(destinations, &results)?;
                    }
                }
                call_effects()
            }
            TerminatorKind::SpawnEmpty { target } => {
                self.verify_target(*target)?;
                spawn_empty_effects()
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
