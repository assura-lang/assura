//! Core domain checkers.
//!
//! AxiomaticDefChecker, OpaqueFunctionChecker, TestGenerator,
//! StdlibTypes, CollectionContracts, CrudAuthContracts.

use std::collections::HashMap;
use std::ops::Range;

use crate::{Type, TypeError};

// ===========================================================================
// T077: CORE.4 Axiomatic definitions
// ===========================================================================

/// Validates axiomatic (abstract mathematical) definitions.
///
/// Error codes:
/// - A31001: axiom references undefined symbol
/// - A31002: axiom set is inconsistent (circular or contradictory)
/// - A31003: axiom not used in any proof
#[derive(Debug, Clone)]
pub(crate) struct AxiomaticDefChecker {
    axioms: HashMap<String, AxiomDef>,
    used_axioms: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct AxiomDef {
    pub name: String,
    pub span: Range<usize>,
    pub references: Vec<String>,
}

impl AxiomaticDefChecker {
    pub fn new() -> Self {
        Self {
            axioms: HashMap::new(),
            used_axioms: Vec::new(),
        }
    }

    pub fn declare_axiom(&mut self, axiom: AxiomDef) {
        self.axioms.insert(axiom.name.clone(), axiom);
    }

    pub fn mark_used(&mut self, name: &str) {
        if !self.used_axioms.contains(&name.to_string()) {
            self.used_axioms.push(name.to_string());
        }
    }

    pub fn check_references(&self, known_symbols: &[&str]) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for axiom in self.axioms.values() {
            for reference in &axiom.references {
                let is_axiom = self.axioms.contains_key(reference);
                let is_known = known_symbols.contains(&reference.as_str());
                if !is_axiom && !is_known {
                    errors.push(TypeError {
                        code: "A31001".into(),
                        message: format!(
                            "axiom `{}` references undefined symbol `{reference}`",
                            axiom.name
                        ),
                        span: axiom.span.clone(),
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_unused(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, axiom) in &self.axioms {
            if !self.used_axioms.contains(name) {
                errors.push(TypeError {
                    code: "A31003".into(),
                    message: format!("axiom `{name}` is never used in any proof"),
                    span: axiom.span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_circular(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, axiom) in &self.axioms {
            if self.has_cycle(name, &mut vec![name.clone()]) {
                errors.push(TypeError {
                    code: "A31002".into(),
                    message: format!("axiom `{name}` has circular dependency"),
                    span: axiom.span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    fn has_cycle(&self, current: &str, visited: &mut Vec<String>) -> bool {
        if let Some(axiom) = self.axioms.get(current) {
            for reference in &axiom.references {
                if visited.contains(reference) {
                    return true;
                }
                if self.axioms.contains_key(reference) {
                    visited.push(reference.clone());
                    if self.has_cycle(reference, visited) {
                        return true;
                    }
                    visited.pop();
                }
            }
        }
        false
    }
}

impl Default for AxiomaticDefChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T079: CORE.6 Opaque functions
// ===========================================================================

/// Manages opaque function declarations that hide implementation from verifier.
///
/// Error codes:
/// - A32001: opaque function called without contract
/// - A32002: opaque function body accessed during verification
/// - A32003: reveal used outside proof context
#[derive(Debug, Clone)]
pub(crate) struct OpaqueFunctionChecker {
    opaque_fns: HashMap<String, OpaqueFnInfo>,
    revealed: Vec<String>,
    in_proof_context: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct OpaqueFnInfo {
    pub has_contract: bool,
    pub span: Range<usize>,
}

impl OpaqueFunctionChecker {
    pub fn new() -> Self {
        Self {
            opaque_fns: HashMap::new(),
            revealed: Vec::new(),
            in_proof_context: false,
        }
    }

    pub fn declare_opaque(&mut self, name: String, has_contract: bool, span: Range<usize>) {
        self.opaque_fns
            .insert(name, OpaqueFnInfo { has_contract, span });
    }

    pub fn enter_proof(&mut self) {
        self.in_proof_context = true;
    }

    pub fn exit_proof(&mut self) {
        self.in_proof_context = false;
    }

    /// Get the declaration span of an opaque function for diagnostics.
    pub fn opaque_span(&self, fn_name: &str) -> Option<&Range<usize>> {
        self.opaque_fns.get(fn_name).map(|i| &i.span)
    }

    pub fn check_call(&self, fn_name: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(info) = self.opaque_fns.get(fn_name)
            && !info.has_contract
        {
            return Some(TypeError {
                code: "A32001".into(),
                message: format!("opaque function `{fn_name}` called without contract"),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_body_access(&self, fn_name: &str, span: &Range<usize>) -> Option<TypeError> {
        if self.opaque_fns.contains_key(fn_name) && !self.revealed.contains(&fn_name.to_string()) {
            return Some(TypeError {
                code: "A32002".into(),
                message: format!("body of opaque function `{fn_name}` accessed without reveal"),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn reveal(&mut self, fn_name: &str, span: &Range<usize>) -> Option<TypeError> {
        if !self.in_proof_context {
            return Some(TypeError {
                code: "A32003".into(),
                message: format!("`reveal {fn_name}` used outside proof context"),
                span: span.clone(),
                secondary: None,
            });
        }
        self.revealed.push(fn_name.to_string());
        None
    }

    pub fn is_opaque(&self, fn_name: &str) -> bool {
        self.opaque_fns.contains_key(fn_name)
    }
}

impl Default for OpaqueFunctionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T083: TEST.1 Test generation from contracts
// ===========================================================================

/// Generates property-based and boundary-value tests from contract specs.
///
/// Produces Rust test code (proptest/quickcheck) from requires/ensures clauses.
#[derive(Debug, Clone)]
pub struct TestGenerator {
    contracts: Vec<TestableContract>,
}

#[derive(Debug, Clone)]
pub struct TestableContract {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub requires: Vec<String>,
    pub ensures: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GeneratedTest {
    pub name: String,
    pub body: String,
    pub kind: TestKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TestKind {
    Property,
    Boundary,
    Smoke,
}

impl TestGenerator {
    pub fn new() -> Self {
        Self {
            contracts: Vec::new(),
        }
    }

    pub fn add_contract(&mut self, contract: TestableContract) {
        self.contracts.push(contract);
    }

    pub fn generate_property_test(&self, contract: &TestableContract) -> GeneratedTest {
        let param_list: Vec<String> = contract
            .params
            .iter()
            .map(|(n, t)| format!("{n}: {}", Self::type_to_proptest_strategy(t)))
            .collect();
        let preconditions = if contract.requires.is_empty() {
            String::new()
        } else {
            format!(
                "prop_assume!({});\n        ",
                contract.requires.join(" && ")
            )
        };
        let postconditions = contract.ensures.join(" && ");
        let body = format!(
            "proptest! {{\n    #[test]\n    fn prop_{}({}) {{\n        {preconditions}prop_assert!({postconditions});\n    }}\n}}",
            contract.name,
            param_list.join(", ")
        );
        GeneratedTest {
            name: format!("prop_{}", contract.name),
            body,
            kind: TestKind::Property,
        }
    }

    pub fn generate_boundary_tests(&self, contract: &TestableContract) -> Vec<GeneratedTest> {
        let mut tests = Vec::new();
        for (name, ty) in &contract.params {
            let boundaries = Self::boundary_values(ty);
            for (i, val) in boundaries.iter().enumerate() {
                tests.push(GeneratedTest {
                    name: format!("boundary_{}_{}_{}", contract.name, name, i),
                    body: format!("#[test]\nfn boundary_{}_{}_{i}() {{\n    let {name} = {val};\n    // boundary test for {name}\n}}", contract.name, name),
                    kind: TestKind::Boundary,
                });
            }
        }
        tests
    }

    pub fn generate_smoke_test(&self, contract: &TestableContract) -> GeneratedTest {
        let body = format!(
            "#[test]\nfn smoke_{}() {{\n    // smoke test: basic valid inputs\n}}",
            contract.name
        );
        GeneratedTest {
            name: format!("smoke_{}", contract.name),
            body,
            kind: TestKind::Smoke,
        }
    }

    pub fn generate_all(&self) -> Vec<GeneratedTest> {
        let mut tests = Vec::new();
        for contract in &self.contracts {
            tests.push(self.generate_property_test(contract));
            tests.extend(self.generate_boundary_tests(contract));
            tests.push(self.generate_smoke_test(contract));
        }
        tests
    }

    fn type_to_proptest_strategy(ty: &Type) -> &'static str {
        match ty {
            Type::Int | Type::I64 => "i64::ANY",
            Type::Nat | Type::U64 => "u64::ANY",
            Type::U8 => "u8::ANY",
            Type::U16 => "u16::ANY",
            Type::U32 => "u32::ANY",
            Type::I8 => "i8::ANY",
            Type::I16 => "i16::ANY",
            Type::I32 => "i32::ANY",
            Type::Float | Type::F64 => "f64::ANY",
            Type::F32 => "f32::ANY",
            Type::Bool => "bool::ANY",
            Type::String => "\".*\"",
            _ => "any::<()>()",
        }
    }

    fn boundary_values(ty: &Type) -> Vec<String> {
        match ty {
            Type::Int | Type::I64 => vec![
                "0".into(),
                "1".into(),
                "-1".into(),
                "i64::MAX".into(),
                "i64::MIN".into(),
            ],
            Type::Nat | Type::U64 => vec!["0".into(), "1".into(), "u64::MAX".into()],
            Type::U8 => vec!["0u8".into(), "1u8".into(), "255u8".into()],
            Type::U16 => vec!["0u16".into(), "1u16".into(), "65535u16".into()],
            Type::U32 => vec!["0u32".into(), "1u32".into(), "u32::MAX".into()],
            Type::I8 => vec![
                "0i8".into(),
                "1i8".into(),
                "-1i8".into(),
                "127i8".into(),
                "-128i8".into(),
            ],
            Type::I16 => vec![
                "0i16".into(),
                "1i16".into(),
                "-1i16".into(),
                "i16::MAX".into(),
                "i16::MIN".into(),
            ],
            Type::I32 => vec![
                "0i32".into(),
                "1i32".into(),
                "-1i32".into(),
                "i32::MAX".into(),
                "i32::MIN".into(),
            ],
            Type::Bool => vec!["true".into(), "false".into()],
            Type::Float | Type::F64 => vec![
                "0.0".into(),
                "1.0".into(),
                "-1.0".into(),
                "f64::INFINITY".into(),
                "f64::NAN".into(),
            ],
            _ => vec![],
        }
    }
}

impl Default for TestGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------

// ===========================================================================
// CORE.5 Quantifier Trigger validation
// ===========================================================================

/// Validates quantifier trigger annotations for verification performance.
///
/// Error codes:
/// - A53006: quantifier has no trigger annotation
/// - A53007: trigger references variable not bound by the quantifier
/// - A53008: trigger term is a sub-expression of the quantifier body (matching loop risk)
#[derive(Debug, Clone)]
pub struct QuantifierTriggerChecker {
    quantifiers: Vec<QuantifierInfo>,
}

#[derive(Debug, Clone)]
struct QuantifierInfo {
    var: String,
    has_trigger: bool,
    span: Range<usize>,
}

impl QuantifierTriggerChecker {
    pub fn new() -> Self {
        Self {
            quantifiers: Vec::new(),
        }
    }

    /// Register a quantifier expression found in a clause body.
    /// `has_trigger` indicates whether a trigger annotation (e.g., `triggers { ... }`)
    /// was found on this quantifier. Currently we detect trigger annotations by
    /// checking if the quantifier domain or body contains a `triggers` identifier.
    pub fn add_quantifier(&mut self, var: String, has_trigger: bool, span: Range<usize>) {
        self.quantifiers.push(QuantifierInfo {
            var,
            has_trigger,
            span,
        });
    }

    /// Check that all quantifiers have trigger annotations.
    /// Returns errors for quantifiers missing triggers.
    pub fn check_triggers(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for q in &self.quantifiers {
            if !q.has_trigger {
                errors.push(TypeError {
                    code: "A53006".into(),
                    message: format!(
                        "quantifier over `{}` has no trigger annotation; \
                         add a `triggers` clause for verification performance",
                        q.var
                    ),
                    span: q.span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }
}

impl Default for QuantifierTriggerChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------

// ===========================================================================
// T107: Core standard library types
// ===========================================================================

/// Core standard library type definitions (Pos, NonNeg, Email, Uuid, etc.)
#[derive(Debug, Clone)]
pub(crate) struct StdlibTypes {
    types: std::collections::HashMap<String, StdlibTypeDef>,
}

#[derive(Debug, Clone)]
pub(crate) struct StdlibTypeDef {
    pub name: String,
    pub base_type: Type,
}

impl StdlibTypes {
    pub fn new() -> Self {
        let mut types = std::collections::HashMap::new();
        types.insert(
            "Pos".into(),
            StdlibTypeDef {
                name: "Pos".into(),
                base_type: Type::Int,
            },
        );
        types.insert(
            "NonNeg".into(),
            StdlibTypeDef {
                name: "NonNeg".into(),
                base_type: Type::Int,
            },
        );
        types.insert(
            "Email".into(),
            StdlibTypeDef {
                name: "Email".into(),
                base_type: Type::String,
            },
        );
        types.insert(
            "Uuid".into(),
            StdlibTypeDef {
                name: "Uuid".into(),
                base_type: Type::String,
            },
        );
        types.insert(
            "Port".into(),
            StdlibTypeDef {
                name: "Port".into(),
                base_type: Type::Int,
            },
        );
        types.insert(
            "Percentage".into(),
            StdlibTypeDef {
                name: "Percentage".into(),
                base_type: Type::Float,
            },
        );
        Self { types }
    }

    pub fn all_types(&self) -> Vec<&StdlibTypeDef> {
        self.types.values().collect()
    }
}

impl Default for StdlibTypes {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T108: Collection contracts (ListOps, sort, filter)
// ===========================================================================

/// Standard collection operation contracts.
#[derive(Debug, Clone)]
pub(crate) struct CollectionContracts {
    contracts: Vec<CollectionContract>,
}

#[derive(Debug, Clone)]
pub(crate) struct CollectionContract {
    pub name: String,
    pub preserves_length: bool,
}

impl CollectionContracts {
    pub fn new() -> Self {
        let contracts = vec![
            CollectionContract {
                name: "sort".into(),
                preserves_length: true,
            },
            CollectionContract {
                name: "filter".into(),
                preserves_length: false,
            },
            CollectionContract {
                name: "map".into(),
                preserves_length: true,
            },
            CollectionContract {
                name: "reverse".into(),
                preserves_length: true,
            },
            CollectionContract {
                name: "deduplicate".into(),
                preserves_length: false,
            },
        ];
        Self { contracts }
    }

    pub fn lookup(&self, name: &str) -> Option<&CollectionContract> {
        self.contracts.iter().find(|c| c.name == name)
    }
}

impl Default for CollectionContracts {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T109: CRUD patterns and auth contracts
// ===========================================================================

/// Standard CRUD and authentication contract patterns.
#[derive(Debug, Clone)]
pub(crate) struct CrudAuthContracts {
    crud_ops: Vec<CrudOperation>,
    auth_policies: Vec<AuthPolicy>,
}

#[derive(Debug, Clone)]
pub(crate) struct CrudOperation {
    pub name: String,
    pub op_type: CrudType,
    pub requires_auth: bool,
    pub preconditions: Vec<String>,
    pub postconditions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CrudType {
    Create,
    Read,
    Update,
    Delete,
}

#[derive(Debug, Clone)]
pub(crate) struct AuthPolicy {
    pub name: String,
    pub required_role: String,
    pub allow_self: bool,
}

impl CrudAuthContracts {
    pub fn new() -> Self {
        Self {
            crud_ops: Vec::new(),
            auth_policies: Vec::new(),
        }
    }

    pub fn add_crud(&mut self, name: String, op_type: CrudType, requires_auth: bool) {
        self.crud_ops.push(CrudOperation {
            name,
            op_type,
            requires_auth,
            preconditions: Vec::new(),
            postconditions: Vec::new(),
        });
    }

    pub fn add_auth_policy(&mut self, name: String, required_role: String, allow_self: bool) {
        self.auth_policies.push(AuthPolicy {
            name,
            required_role,
            allow_self,
        });
    }

    pub fn check_auth_coverage(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for op in &self.crud_ops {
            if op.requires_auth {
                let has_policy = self.auth_policies.iter().any(|p| p.name == op.name);
                if !has_policy {
                    errors.push(TypeError {
                        code: "A53001".into(),
                        message: format!(
                            "CRUD operation `{}` requires auth but has no policy",
                            op.name
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_delete_protection(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for op in &self.crud_ops {
            if op.op_type == CrudType::Delete && !op.requires_auth {
                errors.push(TypeError {
                    code: "A53002".into(),
                    message: format!(
                        "delete operation `{}` should require authentication",
                        op.name
                    ),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    /// Check that CRUD operations with preconditions have matching policies.
    pub fn check_precondition_coverage(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for op in &self.crud_ops {
            if !op.preconditions.is_empty() || !op.postconditions.is_empty() {
                let has_policy = self
                    .auth_policies
                    .iter()
                    .any(|p| p.name == op.name && (!p.required_role.is_empty() || p.allow_self));
                if !has_policy && op.requires_auth {
                    errors.push(TypeError {
                        code: "A53003".into(),
                        message: format!(
                            "CRUD operation `{}` has contracts but no matching auth policy",
                            op.name
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }
}

impl Default for CrudAuthContracts {
    fn default() -> Self {
        Self::new()
    }
}
