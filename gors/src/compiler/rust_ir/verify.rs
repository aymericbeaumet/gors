//! Verification for explicit Rust representation, ABI, storage, and control plans.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::compiler::Diagnostic;
use crate::compiler::ids::QualifiedDefId;
use crate::compiler::provenance::SourceRef;
use gors_runtime_abi::{RuntimeSignature, RuntimeType};

impl File {
    #[cfg(test)]
    pub(super) fn verify(&self) -> Result<RuntimeRequirement, Diagnostic> {
        let signatures = self
            .functions
            .iter()
            .map(|function| {
                (
                    QualifiedDefId::new(self.package_id, function.id),
                    function.signature.clone(),
                )
            })
            .collect();
        self.verify_with_signatures(&signatures)
    }

    pub(super) fn verify_with_signatures(
        &self,
        signatures: &BTreeMap<QualifiedDefId, Signature>,
    ) -> Result<RuntimeRequirement, Diagnostic> {
        let entrypoints = self
            .functions
            .iter()
            .filter(|function| function.artifact.entrypoint == EntrypointPlan::Executable)
            .count();
        if entrypoints > 1 {
            return Err(Diagnostic::backend(
                "multiple Rust IR executable entrypoints",
            ));
        }
        if entrypoints == 1 && self.package != "main" {
            return Err(Diagnostic::backend(
                "Rust IR executable entrypoint belongs to a non-main package",
            ));
        }

        let mut local_signatures = BTreeMap::new();
        let mut symbols = BTreeSet::new();
        for function in &self.functions {
            function.verify_artifact_plan()?;
            if local_signatures
                .insert(
                    QualifiedDefId::new(self.package_id, function.id),
                    function.signature.clone(),
                )
                .is_some()
            {
                return Err(Diagnostic::backend(format!(
                    "duplicate Rust IR function DefId {}",
                    function.id
                )));
            }
            if !symbols.insert(function.artifact.symbol.as_str()) {
                return Err(Diagnostic::backend(format!(
                    "duplicate Rust IR function symbol {}",
                    function.artifact.symbol.as_str()
                )));
            }
        }
        for (definition, signature) in &local_signatures {
            if signatures.get(definition) != Some(signature) {
                return Err(Diagnostic::backend(format!(
                    "Rust IR signature index disagrees with function {:?}",
                    definition
                )));
            }
        }
        let mut requirement = RuntimeRequirement::default();
        for function in &self.functions {
            let owner = QualifiedDefId::new(self.package_id, function.id);
            requirement = requirement.union(&verify_function(function, owner, signatures)?);
        }
        Ok(requirement)
    }
}

pub(super) fn verify_function(
    function: &Function,
    owner: QualifiedDefId,
    signatures: &BTreeMap<QualifiedDefId, Signature>,
) -> Result<RuntimeRequirement, Diagnostic> {
    function.verify_artifact_plan()?;
    if owner.definition() != function.id {
        return Err(Diagnostic::backend(format!(
            "Rust IR owner {owner:?} does not identify function DefId {}",
            function.id
        )));
    }
    let Some(indexed) = signatures.get(&owner) else {
        return Err(Diagnostic::backend(format!(
            "Rust IR signature index is missing function DefId {}",
            function.id
        )));
    };
    if indexed != &function.signature {
        return Err(Diagnostic::backend(format!(
            "Rust IR signature index disagrees with function DefId {}",
            function.id
        )));
    }
    function.verify(signatures)
}

impl Function {
    fn verify(
        &self,
        signatures: &BTreeMap<QualifiedDefId, Signature>,
    ) -> Result<RuntimeRequirement, Diagnostic> {
        verify_source_ref(self.source, self.id, "function")?;
        if self.entry.0 as usize >= self.blocks.len() {
            return Err(Diagnostic::backend("Rust IR entry block does not exist"));
        }
        if self.parameters.len() != self.signature.params.len() {
            return Err(Diagnostic::backend(
                "Rust IR parameter list does not match signature",
            ));
        }
        for (index, local) in self.locals.iter().enumerate() {
            if local.id.0 as usize != index {
                return Err(Diagnostic::backend(format!(
                    "Rust IR local IDs are not dense: expected {index}, found {}",
                    local.id.0
                )));
            }
            if local.storage != StorageClass::CheckedOptionSlot {
                return Err(Diagnostic::backend("unsupported Rust IR storage class"));
            }
            let expected_initialization = self
                .parameters
                .iter()
                .position(|parameter| *parameter == local.id)
                .map_or(
                    SlotInitialization::Uninitialized,
                    SlotInitialization::Parameter,
                );
            if local.initialization != expected_initialization {
                return Err(Diagnostic::backend(format!(
                    "Rust IR slot initialization mismatch for local {}",
                    local.id.0
                )));
            }
        }
        let mut parameter_ids = BTreeSet::new();
        for (position, (parameter, expected)) in self
            .parameters
            .iter()
            .zip(&self.signature.params)
            .enumerate()
        {
            if !parameter_ids.insert(*parameter) {
                return Err(Diagnostic::backend(format!(
                    "Rust IR repeats parameter local {}",
                    parameter.0
                )));
            }
            let local = self.local(*parameter)?;
            if local.initialization != SlotInitialization::Parameter(position) {
                return Err(Diagnostic::backend(format!(
                    "Rust IR parameter {position} has the wrong slot initializer"
                )));
            }
            verify_same(local.ty, *expected, "parameter")?;
        }
        for (index, block) in self.blocks.iter().enumerate() {
            if block.id.0 as usize != index {
                return Err(Diagnostic::backend(format!(
                    "Rust IR block IDs are not dense: expected {index}, found {}",
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
        self.verify_control_flow_plan()?;
        self.verify_storage_dataflow()?;
        Ok(runtime_requirement(self))
    }

    fn verify_panic_cleanup(&self) -> Result<(), Diagnostic> {
        if let Some(cleanup) = self.panic_cleanup {
            self.verify_target(cleanup.entry)?;
            verify_same(
                self.place_ty(Place {
                    local: cleanup.active,
                })?,
                RustType::Bool,
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
                        "Rust IR panic edge does not target the function cleanup entry",
                    ));
                }
            }
        }
        Ok(())
    }

    fn verify_control_flow_plan(&self) -> Result<(), Diagnostic> {
        match &self.control_flow {
            ControlFlowPlan::PcDispatchU32 => Ok(()),
            ControlFlowPlan::StructuredLinear { order } => {
                let expected = super::idiom::recognize_linear_order(self)?.ok_or_else(|| {
                    Diagnostic::backend(
                        "Rust IR structured-linear plan does not match a straight-line CFG",
                    )
                })?;
                if order != &expected {
                    return Err(Diagnostic::backend(
                        "Rust IR structured-linear plan block order is not canonical",
                    ));
                }
                Ok(())
            }
        }
    }

    fn verify_artifact_plan(&self) -> Result<(), Diagnostic> {
        match self.artifact.entrypoint {
            EntrypointPlan::Executable => {
                let expected = FunctionArtifactPlan::executable_entrypoint();
                if self.artifact.symbol != expected.symbol {
                    return Err(Diagnostic::backend(
                        "Rust IR executable entrypoint must use the canonical main symbol",
                    ));
                }
                if self.artifact.linkage != expected.linkage {
                    return Err(Diagnostic::backend(
                        "Rust IR executable entrypoint must use internal linkage",
                    ));
                }
                if !self.signature.params.is_empty() || !self.signature.results.is_empty() {
                    return Err(Diagnostic::backend(
                        "Rust IR executable entrypoint must have no parameters or results",
                    ));
                }
            }
            EntrypointPlan::None => {
                let expected = FunctionArtifactPlan::public_definition(self.id);
                if self.artifact.symbol != expected.symbol {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR function DefId {} does not use its canonical symbol",
                        self.id
                    )));
                }
                if self.artifact.linkage != expected.linkage {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR function DefId {} must use public linkage",
                        self.id
                    )));
                }
            }
        }
        Ok(())
    }

    fn verify_statement(&self, statement: &Statement) -> Result<(), Diagnostic> {
        if statement.store != StoreOp::SetSome {
            return Err(Diagnostic::backend("unsupported Rust IR store operation"));
        }
        verify_statement_provenance(&statement.provenance, self.id)?;
        let destination = self.place_ty(statement.destination)?;
        let value = self.rvalue_ty(&statement.value)?;
        verify_same(value, destination, "assignment")?;
        verify_effects(
            statement.effects,
            statement_effects(&statement.value),
            "statement",
        )
    }

    fn rvalue_ty(&self, rvalue: &Rvalue) -> Result<RustType, Diagnostic> {
        verify_rvalue_provenance(&rvalue.provenance, self.id)?;
        let result = match &rvalue.kind {
            RvalueKind::Use(operand) => self.operand_ty(operand)?,
            RvalueKind::Unary { op, operand } => {
                let operand = self.operand_ty(operand)?;
                verify_value_operation(*op, &[operand], "unary operation")?
            }
            RvalueKind::RecoverCompareNil { state, .. } => {
                verify_same(
                    self.place_ty(*state)?,
                    RustType::Bool,
                    "panic recovery state",
                )?;
                RustType::Bool
            }
            RvalueKind::Binary { op, left, right } => {
                let left = self.operand_ty(left)?;
                let right = self.operand_ty(right)?;
                verify_value_operation(*op, &[left, right], "binary operation")?
            }
            RvalueKind::ArrayIndexI64 { array, index } => {
                let array = self.operand_ty(array)?;
                let index = self.operand_ty(index)?;
                if !matches!(array, RustType::ArrayI64(_)) || index != RustType::I64 {
                    return Err(Diagnostic::backend(format!(
                        "invalid Rust IR array index types: {array:?}[{index:?}]"
                    )));
                }
                RustType::I64
            }
            RvalueKind::ArrayIndex { array, index } => {
                let array = self.operand_ty(array)?;
                let index = self.operand_ty(index)?;
                let Some((_, element)) = array.scalar_array_parts() else {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR scalar array index has a non-array operand: {array:?}"
                    )));
                };
                verify_same(index, RustType::I64, "scalar array index")?;
                element
            }
            RvalueKind::ArraySetI64 {
                array,
                index,
                value,
            } => {
                let array = self.operand_ty(array)?;
                let index = self.operand_ty(index)?;
                let value = self.operand_ty(value)?;
                if !matches!(array, RustType::ArrayI64(_))
                    || index != RustType::I64
                    || value != RustType::I64
                {
                    return Err(Diagnostic::backend(format!(
                        "invalid Rust IR array update types: {array:?}[{index:?}] = {value:?}"
                    )));
                }
                array
            }
            RvalueKind::ArraySet {
                array,
                index,
                value,
            } => {
                let array = self.operand_ty(array)?;
                let index = self.operand_ty(index)?;
                let value = self.operand_ty(value)?;
                let Some((_, element)) = array.scalar_array_parts() else {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR scalar array update has a non-array operand: {array:?}"
                    )));
                };
                verify_same(index, RustType::I64, "scalar array update index")?;
                verify_same(value, element, "scalar array update value")?;
                array
            }
            RvalueKind::ArrayLiteral { elements, ty } => {
                let Some((length, element_ty)) = ty.scalar_array_parts() else {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR scalar array literal has a non-array type: {ty:?}"
                    )));
                };
                let actual_length = u64::try_from(elements.len()).map_err(|_| {
                    Diagnostic::backend("Rust IR scalar array literal length does not fit u64")
                })?;
                if actual_length != length {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR scalar array literal has {actual_length} elements for length {length}"
                    )));
                }
                for element in elements {
                    verify_same(
                        self.operand_ty(element)?,
                        element_ty,
                        "scalar array literal element",
                    )?;
                }
                *ty
            }
            RvalueKind::StructLiteralI64(fields) => {
                for field in fields {
                    verify_same(
                        self.operand_ty(field)?,
                        RustType::I64,
                        "struct literal field",
                    )?;
                }
                RustType::StructI64(u64::try_from(fields.len()).map_err(|_| {
                    Diagnostic::backend("Rust IR struct field count does not fit u64")
                })?)
            }
            RvalueKind::StructFieldI64 { structure, field } => {
                let structure = self.operand_ty(structure)?;
                let RustType::StructI64(length) = structure else {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR field read has a non-struct operand: {structure:?}"
                    )));
                };
                if u64::from(*field) >= length {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR struct field index {field} is outside {length} fields"
                    )));
                }
                RustType::I64
            }
            RvalueKind::StructSetI64 {
                structure,
                field,
                value,
            } => {
                let structure = self.operand_ty(structure)?;
                let RustType::StructI64(length) = structure else {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR field update has a non-struct operand: {structure:?}"
                    )));
                };
                if u64::from(*field) >= length {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR struct field update {field} is outside {length} fields"
                    )));
                }
                verify_same(
                    self.operand_ty(value)?,
                    RustType::I64,
                    "struct field update",
                )?;
                structure
            }
            RvalueKind::AggregateEqualI64 { left, right, .. } => {
                let left = self.operand_ty(left)?;
                let right = self.operand_ty(right)?;
                if left != right || !matches!(left, RustType::ArrayI64(_) | RustType::StructI64(_))
                {
                    return Err(Diagnostic::backend(format!(
                        "invalid Rust IR aggregate equality types: {left:?}, {right:?}"
                    )));
                }
                RustType::Bool
            }
        };
        verify_effects(rvalue.effects, rvalue_effects(&rvalue.kind), "rvalue")?;
        verify_panic(rvalue.effects, rvalue.panic, "rvalue")?;
        Ok(result)
    }

    fn verify_terminator(
        &self,
        terminator: &Terminator,
        signatures: &BTreeMap<QualifiedDefId, Signature>,
    ) -> Result<(), Diagnostic> {
        verify_terminator_provenance(&terminator.provenance, self.id)?;
        match &terminator.kind {
            TerminatorKind::Goto(target) => {
                self.verify_target(*target)?;
            }
            TerminatorKind::SwitchBool {
                condition,
                then_target,
                else_target,
            } => {
                verify_same(
                    self.operand_ty(condition)?,
                    RustType::Bool,
                    "switch condition",
                )?;
                self.verify_target(*then_target)?;
                self.verify_target(*else_target)?;
            }
            TerminatorKind::Call {
                target,
                args,
                destinations,
                next,
            } => {
                self.verify_target(*next)?;
                let argument_types = args
                    .iter()
                    .map(|argument| self.operand_ty(argument))
                    .collect::<Result<Vec<_>, _>>()?;
                match target {
                    CallTarget::Function(id) => {
                        let signature = signatures.get(id).ok_or_else(|| {
                            Diagnostic::backend(format!("unknown Rust IR callee DefId {id}"))
                        })?;
                        if argument_types != signature.params {
                            return Err(Diagnostic::backend(format!(
                                "Rust IR call arguments do not match DefId {}",
                                id
                            )));
                        }
                        self.verify_call_destinations(destinations, &signature.results)?;
                    }
                    CallTarget::Runtime(operation) => {
                        let signature = operation.signature();
                        verify_operation_arguments(signature, &argument_types, "runtime call")?;
                        let results =
                            rust_types_from_runtime_result(signature.result(), "runtime call")?;
                        self.verify_call_destinations(destinations, &results)?;
                    }
                }
            }
            TerminatorKind::Return(values) => {
                if values.len() != self.signature.results.len() {
                    return Err(Diagnostic::backend(
                        "Rust IR return count does not match signature",
                    ));
                }
                for (value, expected) in values.iter().zip(&self.signature.results) {
                    verify_same(self.operand_ty(value)?, *expected, "return value")?;
                }
            }
            TerminatorKind::Unreachable => {}
        }
        verify_effects(
            terminator.effects,
            terminator_effects(&terminator.kind),
            "terminator",
        )?;
        verify_panic(terminator.effects, terminator.panic, "terminator")
    }

    fn verify_call_destinations(
        &self,
        destinations: &[Place],
        results: &[RustType],
    ) -> Result<(), Diagnostic> {
        if destinations.len() != results.len() {
            return Err(Diagnostic::backend(format!(
                "Rust IR call has {} destinations for {} result values",
                destinations.len(),
                results.len()
            )));
        }
        for (destination, result) in destinations.iter().zip(results) {
            verify_same(self.place_ty(*destination)?, *result, "call destination")?;
        }
        Ok(())
    }

    fn verify_target(&self, target: BasicBlockId) -> Result<(), Diagnostic> {
        ((target.0 as usize) < self.blocks.len())
            .then_some(())
            .ok_or_else(|| Diagnostic::backend(format!("invalid Rust IR target {}", target.0)))
    }

    fn local(&self, local: LocalId) -> Result<&LocalDecl, Diagnostic> {
        self.locals
            .get(local.0 as usize)
            .ok_or_else(|| Diagnostic::backend(format!("invalid Rust IR local {}", local.0)))
    }

    fn place_ty(&self, place: Place) -> Result<RustType, Diagnostic> {
        self.local(place.local).map(|local| local.ty)
    }

    fn operand_ty(&self, operand: &Operand) -> Result<RustType, Diagnostic> {
        match operand {
            Operand::Read { place, op } => {
                let ty = self.place_ty(*place)?;
                if !ty.supports_read_op(*op) {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR read operation {op:?} is invalid for {ty:?}"
                    )));
                }
                Ok(ty)
            }
            Operand::Constant(constant) => constant_type(constant),
            Operand::Unit => Ok(RustType::Unit),
        }
    }
}

fn verify_value_operation(
    operation: ValueOp,
    arguments: &[RustType],
    context: &str,
) -> Result<RustType, Diagnostic> {
    let signature = match operation {
        ValueOp::Primitive(operation) => operation.signature(),
        ValueOp::Runtime(operation) => operation.signature(),
    };
    verify_operation_signature(signature, arguments, context)
}

fn verify_operation_signature(
    signature: RuntimeSignature,
    arguments: &[RustType],
    context: &str,
) -> Result<RustType, Diagnostic> {
    verify_operation_arguments(signature, arguments, context)?;
    rust_type_from_runtime(signature.result(), context)
}

fn verify_operation_arguments(
    signature: RuntimeSignature,
    arguments: &[RustType],
    context: &str,
) -> Result<(), Diagnostic> {
    if arguments.len() != signature.parameters().len() {
        return Err(Diagnostic::backend(format!(
            "Rust IR {context} has {} arguments but its ABI signature requires {}",
            arguments.len(),
            signature.parameters().len()
        )));
    }
    for (position, (actual, expected)) in arguments
        .iter()
        .copied()
        .zip(signature.parameters().iter().copied())
        .enumerate()
    {
        let expected = rust_type_from_runtime(expected, context)?;
        verify_same(actual, expected, &format!("{context} argument {position}"))?;
    }
    Ok(())
}

fn rust_types_from_runtime_result(
    ty: RuntimeType,
    context: &str,
) -> Result<Vec<RustType>, Diagnostic> {
    match ty {
        RuntimeType::Unit => Ok(Vec::new()),
        RuntimeType::I64BoolTuple => Ok(vec![RustType::I64, RustType::Bool]),
        RuntimeType::I64I64Tuple => Ok(vec![RustType::I64, RustType::I64]),
        ty => rust_type_from_runtime(ty, context).map(|ty| vec![ty]),
    }
}

fn rust_type_from_runtime(ty: RuntimeType, context: &str) -> Result<RustType, Diagnostic> {
    match ty {
        RuntimeType::Unit => Ok(RustType::Unit),
        RuntimeType::Bool => Ok(RustType::Bool),
        RuntimeType::I64 => Ok(RustType::I64),
        RuntimeType::F64 => Ok(RustType::F64),
        RuntimeType::Complex128 => Ok(RustType::Complex128),
        RuntimeType::GoString => Ok(RustType::GoString),
        RuntimeType::GoSliceI64 => Ok(RustType::GoSliceI64),
        RuntimeType::GoSliceU8 => Ok(RustType::GoSliceU8),
        RuntimeType::GoSliceBool => Ok(RustType::GoSliceBool),
        RuntimeType::GoMapStringI64 => Ok(RustType::GoMapStringI64),
        RuntimeType::GoPointerI64 => Ok(RustType::GoPointerI64),
        RuntimeType::GoPointerStructI64 => Ok(RustType::GoPointerStructI64),
        RuntimeType::GoInterface => Ok(RustType::GoInterface),
        RuntimeType::GoChannelI64 => Ok(RustType::GoChannelI64),
        RuntimeType::ByteSlice
        | RuntimeType::StaticByteSlice
        | RuntimeType::StaticI64Slice
        | RuntimeType::StaticBoolSlice
        | RuntimeType::I64BoolTuple
        | RuntimeType::I64I64Tuple => Err(Diagnostic::backend(format!(
            "Rust IR {context} requires ABI-only operand type {ty:?}"
        ))),
    }
}

fn constant_type(constant: &Constant) -> Result<RustType, Diagnostic> {
    match constant {
        Constant::Bool(_) => Ok(RustType::Bool),
        Constant::I64(_) => Ok(RustType::I64),
        Constant::F64(_) => Ok(RustType::F64),
        Constant::Complex128 { .. } => Ok(RustType::Complex128),
        Constant::StaticI64Array(values) => Ok(RustType::ArrayI64(
            u64::try_from(values.len())
                .map_err(|_| Diagnostic::backend("Rust IR array length does not fit u64"))?,
        )),
        Constant::RuntimeStaticBytes { op, .. } => {
            let signature = op.signature();
            if signature.parameters() == [RuntimeType::StaticByteSlice]
                && signature.result() == RuntimeType::GoString
            {
                Ok(RustType::GoString)
            } else {
                Err(Diagnostic::backend(format!(
                    "Rust IR static bytes use runtime operation {op:?} with incompatible signature {:?} -> {:?}",
                    signature.parameters(),
                    signature.result()
                )))
            }
        }
        Constant::RuntimeStaticI64s { op, .. } => {
            let signature = op.signature();
            if signature.parameters() == [RuntimeType::StaticI64Slice]
                && signature.result() == RuntimeType::GoSliceI64
            {
                Ok(RustType::GoSliceI64)
            } else {
                Err(Diagnostic::backend(format!(
                    "Rust IR static int slice uses runtime operation {op:?} with an incompatible signature"
                )))
            }
        }
        Constant::RuntimeStaticBools { op, .. } => {
            let signature = op.signature();
            if signature.parameters() == [RuntimeType::StaticBoolSlice]
                && signature.result() == RuntimeType::GoSliceBool
            {
                Ok(RustType::GoSliceBool)
            } else {
                Err(Diagnostic::backend(format!(
                    "Rust IR static bool slice uses runtime operation {op:?} with an incompatible signature"
                )))
            }
        }
        Constant::RuntimeStaticU8s { op, .. } => {
            let signature = op.signature();
            if signature.parameters() == [RuntimeType::StaticByteSlice]
                && signature.result() == RuntimeType::GoSliceU8
            {
                Ok(RustType::GoSliceU8)
            } else {
                Err(Diagnostic::backend(format!(
                    "Rust IR static byte slice uses runtime operation {op:?} with an incompatible signature"
                )))
            }
        }
    }
}

fn runtime_requirement(function: &Function) -> RuntimeRequirement {
    let mut operations = Vec::new();
    for block in &function.blocks {
        for statement in &block.statements {
            collect_rvalue_runtime_operations(&statement.value, &mut operations);
        }
        collect_terminator_runtime_operations(&block.terminator, &mut operations);
    }
    RuntimeRequirement::new(operations)
}

fn collect_rvalue_runtime_operations(rvalue: &Rvalue, operations: &mut Vec<RuntimeOp>) {
    match &rvalue.kind {
        RvalueKind::Use(operand) => collect_operand_runtime_operations(operand, operations),
        RvalueKind::Unary { op, operand } => {
            collect_value_runtime_operation(*op, operations);
            collect_operand_runtime_operations(operand, operations);
        }
        RvalueKind::Binary { op, left, right } => {
            collect_value_runtime_operation(*op, operations);
            collect_operand_runtime_operations(left, operations);
            collect_operand_runtime_operations(right, operations);
        }
        RvalueKind::ArrayIndexI64 { array, index } | RvalueKind::ArrayIndex { array, index } => {
            collect_operand_runtime_operations(array, operations);
            collect_operand_runtime_operations(index, operations);
        }
        RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        }
        | RvalueKind::ArraySet {
            array,
            index,
            value,
        } => {
            collect_operand_runtime_operations(array, operations);
            collect_operand_runtime_operations(index, operations);
            collect_operand_runtime_operations(value, operations);
        }
        RvalueKind::ArrayLiteral { elements, .. } => {
            for element in elements {
                collect_operand_runtime_operations(element, operations);
            }
        }
        RvalueKind::StructLiteralI64(fields) => {
            for field in fields {
                collect_operand_runtime_operations(field, operations);
            }
        }
        RvalueKind::StructFieldI64 { structure, .. } => {
            collect_operand_runtime_operations(structure, operations);
        }
        RvalueKind::StructSetI64 {
            structure, value, ..
        } => {
            collect_operand_runtime_operations(structure, operations);
            collect_operand_runtime_operations(value, operations);
        }
        RvalueKind::AggregateEqualI64 { left, right, .. } => {
            collect_operand_runtime_operations(left, operations);
            collect_operand_runtime_operations(right, operations);
        }
        RvalueKind::RecoverCompareNil { .. } => {}
    }
}

fn collect_terminator_runtime_operations(terminator: &Terminator, operations: &mut Vec<RuntimeOp>) {
    match &terminator.kind {
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => {}
        TerminatorKind::SwitchBool { condition, .. } => {
            collect_operand_runtime_operations(condition, operations);
        }
        TerminatorKind::Call { target, args, .. } => {
            if let CallTarget::Runtime(operation) = target {
                operations.push(*operation);
            }
            for argument in args {
                collect_operand_runtime_operations(argument, operations);
            }
        }
        TerminatorKind::Return(values) => {
            for value in values {
                collect_operand_runtime_operations(value, operations);
            }
        }
    }
}

fn collect_value_runtime_operation(operation: ValueOp, operations: &mut Vec<RuntimeOp>) {
    if let ValueOp::Runtime(operation) = operation {
        operations.push(operation);
    }
}

fn collect_operand_runtime_operations(operand: &Operand, operations: &mut Vec<RuntimeOp>) {
    if let Operand::Constant(
        Constant::RuntimeStaticBytes { op, .. }
        | Constant::RuntimeStaticI64s { op, .. }
        | Constant::RuntimeStaticBools { op, .. }
        | Constant::RuntimeStaticU8s { op, .. },
    ) = operand
    {
        operations.push(*op);
    }
}

fn verify_effects(actual: Effects, expected: Effects, context: &str) -> Result<(), Diagnostic> {
    (actual == expected).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "Rust IR {context} effect mismatch: expected {expected:?}, found {actual:?}"
        ))
    })
}

fn verify_panic(effects: Effects, edge: PanicEdge, context: &str) -> Result<(), Diagnostic> {
    let valid = if effects.may_panic {
        matches!(edge, PanicEdge::Propagate | PanicEdge::Cleanup(_))
    } else {
        edge == PanicEdge::None
    };
    valid.then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "Rust IR {context} panic edge mismatch for effects {effects:?}: found {edge:?}"
        ))
    })
}

fn verify_same(actual: RustType, expected: RustType, context: &str) -> Result<(), Diagnostic> {
    (actual == expected).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "Rust IR {context} type mismatch: expected {expected:?}, found {actual:?}"
        ))
    })
}

fn verify_source_provenance(
    provenance: &Provenance,
    owner: DefId,
    context: &str,
) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(source) => verify_source_ref(*source, owner, context),
        Provenance::Synthetic(
            SyntheticOrigin::PanicCleanupDispatch | SyntheticOrigin::ZeroValueCall,
        ) => Ok(()),
        Provenance::Synthetic(origin) => Err(Diagnostic::backend(format!(
            "synthetic provenance {origin:?} is invalid for {context}"
        ))),
    }
}

fn verify_statement_provenance(provenance: &Provenance, owner: DefId) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(source) => verify_source_ref(*source, owner, "statement"),
        Provenance::Synthetic(
            SyntheticOrigin::NamedResultInitialization
            | SyntheticOrigin::PanicCleanupInitialization,
        ) => Ok(()),
        Provenance::Synthetic(other) => Err(Diagnostic::backend(format!(
            "synthetic provenance {other:?} is invalid for a statement"
        ))),
    }
}

fn verify_rvalue_provenance(provenance: &Provenance, owner: DefId) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(source) => verify_source_ref(*source, owner, "rvalue"),
        Provenance::Synthetic(
            SyntheticOrigin::NamedResultInitialization
            | SyntheticOrigin::PanicCleanupInitialization,
        ) => Ok(()),
        Provenance::Synthetic(other) => Err(Diagnostic::backend(format!(
            "synthetic provenance {other:?} is invalid for an rvalue"
        ))),
    }
}

fn verify_terminator_provenance(provenance: &Provenance, owner: DefId) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(source) => verify_source_ref(*source, owner, "terminator"),
        Provenance::Synthetic(
            SyntheticOrigin::ImplicitReturn
            | SyntheticOrigin::PanicCleanupDispatch
            | SyntheticOrigin::ZeroValueCall,
        ) => Ok(()),
        Provenance::Synthetic(other) => Err(Diagnostic::backend(format!(
            "synthetic provenance {other:?} is invalid for a terminator"
        ))),
    }
}

fn verify_source_ref(source: SourceRef, owner: DefId, context: &str) -> Result<(), Diagnostic> {
    (source.owner() == owner).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "Rust IR {context} source reference is owned by DefId {}, expected {owner}",
            source.owner()
        ))
    })
}
