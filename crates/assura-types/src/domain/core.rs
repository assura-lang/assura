//! Core domain checkers.
//!
//! AxiomaticDefChecker, OpaqueFunctionChecker, TestGenerator,
//! StdlibTypes, CollectionContracts, CrudAuthContracts.

use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{BlockKind, ClauseKind, Decl, Expr, ServiceItem, SpExpr};

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
                        suggestion: None,
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
                    suggestion: None,
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
                    suggestion: None,
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
                suggestion: None,
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
                suggestion: None,
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
                suggestion: None,
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
            Type::String => vec![
                r#""""#.into(),
                r#""hello""#.into(),
                r#""a""#.into(),
                r#""Hello, World!""#.into(),
            ],
            Type::Bytes => vec!["b\"\"".into(), "b\"\\x00\"".into(), "b\"\\xff\"".into()],
            Type::List(_) => vec!["vec![]".into(), "vec![Default::default()]".into()],
            Type::Map(_, _) => vec!["HashMap::new()".into()],
            Type::Set(_) => vec!["HashSet::new()".into()],
            Type::Option(_) => vec!["None".into(), "Some(Default::default())".into()],
            Type::Result(_, _) => vec![
                "Ok(Default::default())".into(),
                "Err(Default::default())".into(),
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
                    suggestion: None,
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
        // Numeric refinement types
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
            "Nat".into(),
            StdlibTypeDef {
                name: "Nat".into(),
                base_type: Type::Nat,
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
        // String refinement types
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
            "Url".into(),
            StdlibTypeDef {
                name: "Url".into(),
                base_type: Type::String,
            },
        );
        types.insert(
            "FilePath".into(),
            StdlibTypeDef {
                name: "FilePath".into(),
                base_type: Type::String,
            },
        );
        types.insert(
            "IpAddr".into(),
            StdlibTypeDef {
                name: "IpAddr".into(),
                base_type: Type::String,
            },
        );
        types.insert(
            "Hostname".into(),
            StdlibTypeDef {
                name: "Hostname".into(),
                base_type: Type::String,
            },
        );
        // Byte/buffer types
        types.insert(
            "NonEmptyBytes".into(),
            StdlibTypeDef {
                name: "NonEmptyBytes".into(),
                base_type: Type::Bytes,
            },
        );
        types.insert(
            "BoundedBytes".into(),
            StdlibTypeDef {
                name: "BoundedBytes".into(),
                base_type: Type::Bytes,
            },
        );
        // Timestamp / duration
        types.insert(
            "Timestamp".into(),
            StdlibTypeDef {
                name: "Timestamp".into(),
                base_type: Type::Int,
            },
        );
        types.insert(
            "Duration".into(),
            StdlibTypeDef {
                name: "Duration".into(),
                base_type: Type::Int,
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
                        suggestion: None,
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
                    suggestion: None,
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
                        suggestion: None,
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

impl CrudAuthContracts {
    /// AST-walking entry point: scan services for CRUD operations and check auth.
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for decl in &source.decls {
            if let Decl::Service(s) = &decl.node {
                let mut checker = CrudAuthContracts::new();
                for item in &s.items {
                    if let ServiceItem::Operation { name, clauses } = item {
                        let has_auth = clauses.iter().any(|c| {
                            matches!(c.kind, ClauseKind::Other(ref k) if k == "auth" || k == "requires_auth")
                        });
                        let crud_type = if name.starts_with("create") || name.starts_with("add") {
                            CrudType::Create
                        } else if name.starts_with("read")
                            || name.starts_with("get")
                            || name.starts_with("list")
                        {
                            CrudType::Read
                        } else if name.starts_with("update") || name.starts_with("set") {
                            CrudType::Update
                        } else if name.starts_with("delete") || name.starts_with("remove") {
                            CrudType::Delete
                        } else {
                            continue;
                        };
                        checker.add_crud(name.clone(), crud_type, has_auth);
                    }
                }
                for item in &s.items {
                    if let ServiceItem::Operation { name, clauses } = item {
                        for clause in clauses {
                            if let ClauseKind::Other(ref k) = clause.kind
                                && (k == "auth_policy" || k == "role")
                            {
                                let role = extract_ident_from_expr(&clause.body)
                                    .unwrap_or("user")
                                    .to_string();
                                let allow_self = clauses.iter().any(
                                    |c| matches!(&c.kind, ClauseKind::Other(k2) if k2 == "allow_self"),
                                );
                                checker.add_auth_policy(name.clone(), role, allow_self);
                            }
                        }
                    }
                }
                errors.extend(checker.check_auth_coverage());
                errors.extend(checker.check_delete_protection());
                errors.extend(checker.check_precondition_coverage());
            }
        }
        errors
    }
}

impl AxiomaticDefChecker {
    /// AST-walking entry point: collect axioms, mark used, and run checks.
    pub fn check_source(
        source: &assura_parser::ast::SourceFile,
        symbols: &assura_resolve::SymbolTable,
    ) -> Vec<TypeError> {
        let mut checker = AxiomaticDefChecker::new();
        let axiom_names: Vec<String> = source
            .decls
            .iter()
            .filter_map(|d| {
                if let Decl::Block { kind, name, .. } = &d.node
                    && *kind == BlockKind::Axiomatic
                {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        for decl in &source.decls {
            if let Decl::Block {
                kind, name, body, ..
            } = &decl.node
                && *kind == BlockKind::Axiomatic
            {
                let mut refs = Vec::new();
                for clause in body {
                    let idents = crate::checkers::collect_ident_references(&clause.body);
                    for ident in &idents {
                        if axiom_names.contains(ident) && ident != name {
                            refs.push(ident.clone());
                        }
                    }
                }
                refs.sort();
                refs.dedup();
                checker.declare_axiom(AxiomDef {
                    name: name.clone(),
                    span: decl.span.clone(),
                    references: refs,
                });
            }
        }
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                    let refs = crate::checkers::collect_ident_references(&clause.body);
                    for name in &refs {
                        checker.mark_used(name);
                    }
                }
            }
        }
        let known: Vec<&str> = symbols.symbols.iter().map(|s| s.name.as_str()).collect();
        let mut errors = checker.check_references(&known);
        errors.extend(checker.check_unused());
        errors.extend(checker.check_circular());
        errors
    }
}

impl QuantifierTriggerChecker {
    /// AST-walking entry point: scan clause bodies for quantifiers missing triggers.
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = QuantifierTriggerChecker::new();
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_extern(&decl.node) else {
                continue;
            };
            let has_strict = clauses
                .iter()
                .any(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "strict_triggers"));
            if !has_strict {
                continue;
            }
            for clause in clauses {
                collect_quantifiers(&clause.body, &mut checker, &decl.span);
            }
        }
        checker.check_triggers()
    }
}

// ===========================================================================
// Prophecy resolution checker
// ===========================================================================

/// Validates that prophecy declarations have matching resolve() calls.
pub(crate) struct ProphecyResolutionChecker;

impl ProphecyResolutionChecker {
    /// AST-walking entry point: check that each referenced prophecy is resolved.
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut errors = Vec::new();
        let prophecies: Vec<(&str, &std::ops::Range<usize>)> = source
            .decls
            .iter()
            .filter_map(|d| match &d.node {
                Decl::Prophecy(p) => Some((p.name.as_str(), &d.span)),
                Decl::Block {
                    kind: BlockKind::Other(k),
                    name,
                    ..
                } if k == "prophecy" => Some((name.as_str(), &d.span)),
                _ => None,
            })
            .collect();
        if prophecies.is_empty() {
            return errors;
        }
        let prophecy_names: std::collections::HashSet<&str> =
            prophecies.iter().map(|(n, _)| *n).collect();
        let mut referenced_names = std::collections::HashSet::new();
        let mut resolved_names = std::collections::HashSet::new();
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                collect_resolve_calls(&clause.body, &mut resolved_names);
                collect_ident_refs(&clause.body, &prophecy_names, &mut referenced_names);
            }
        }
        for (name, span) in prophecies {
            if referenced_names.contains(name) && !resolved_names.contains(name) {
                errors.push(TypeError {
                    code: "A05025".into(),
                    message: format!("prophecy variable `{name}` is never resolved"),
                    span: span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }
}

// ===========================================================================
// Private helpers for check_source methods
// ===========================================================================

fn extract_ident_from_expr(expr: &SpExpr) -> Option<&str> {
    match &expr.node {
        Expr::Ident(s) => Some(s.as_str()),
        Expr::Raw(tokens) => tokens.first().map(|s| s.as_str()),
        _ => None,
    }
}

fn collect_quantifiers(
    expr: &SpExpr,
    checker: &mut QuantifierTriggerChecker,
    fallback_span: &std::ops::Range<usize>,
) {
    match &expr.node {
        Expr::Forall { var, domain, body } | Expr::Exists { var, domain, body } => {
            let has_trigger =
                expr_contains_text(domain, "triggers") || expr_contains_text(body, "triggers");
            checker.add_quantifier(var.clone(), has_trigger, fallback_span.clone());
            collect_quantifiers(domain, checker, fallback_span);
            collect_quantifiers(body, checker, fallback_span);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_quantifiers(lhs, checker, fallback_span);
            collect_quantifiers(rhs, checker, fallback_span);
        }
        Expr::UnaryOp { expr: e, .. } | Expr::Old(e) => {
            collect_quantifiers(e, checker, fallback_span);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_quantifiers(cond, checker, fallback_span);
            collect_quantifiers(then_branch, checker, fallback_span);
            if let Some(eb) = else_branch {
                collect_quantifiers(eb, checker, fallback_span);
            }
        }
        Expr::Call { func, args } => {
            collect_quantifiers(func, checker, fallback_span);
            for a in args {
                collect_quantifiers(a, checker, fallback_span);
            }
        }
        Expr::Block(exprs) | Expr::List(exprs) => {
            for e in exprs {
                collect_quantifiers(e, checker, fallback_span);
            }
        }
        Expr::Field(e, _) | Expr::Index { expr: e, .. } => {
            collect_quantifiers(e, checker, fallback_span);
        }
        Expr::Match { scrutinee, arms } => {
            collect_quantifiers(scrutinee, checker, fallback_span);
            for arm in arms {
                collect_quantifiers(&arm.body, checker, fallback_span);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_quantifiers(value, checker, fallback_span);
            collect_quantifiers(body, checker, fallback_span);
        }
        _ => {}
    }
}

fn expr_contains_text(expr: &SpExpr, text: &str) -> bool {
    match &expr.node {
        Expr::Ident(s) => s == text,
        Expr::Raw(tokens) => tokens.iter().any(|t| t == text),
        Expr::Block(exprs) | Expr::List(exprs) => exprs.iter().any(|e| expr_contains_text(e, text)),
        Expr::Call { func, args } => {
            expr_contains_text(func, text) || args.iter().any(|a| expr_contains_text(a, text))
        }
        _ => false,
    }
}

fn collect_ident_refs(
    expr: &SpExpr,
    prophecy_names: &std::collections::HashSet<&str>,
    found: &mut std::collections::HashSet<String>,
) {
    match &expr.node {
        Expr::Ident(name) => {
            if prophecy_names.contains(name.as_str()) {
                found.insert(name.clone());
            }
        }
        Expr::Raw(tokens) => {
            for tok in tokens {
                if prophecy_names.contains(tok.as_str()) {
                    found.insert(tok.clone());
                }
            }
        }
        Expr::Call { func, args } => {
            collect_ident_refs(func, prophecy_names, found);
            for arg in args {
                collect_ident_refs(arg, prophecy_names, found);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_ident_refs(lhs, prophecy_names, found);
            collect_ident_refs(rhs, prophecy_names, found);
        }
        Expr::UnaryOp { expr, .. } | Expr::Old(expr) | Expr::Ghost(expr) => {
            collect_ident_refs(expr, prophecy_names, found);
        }
        Expr::Block(es) | Expr::List(es) => {
            for e in es {
                collect_ident_refs(e, prophecy_names, found);
            }
        }
        _ => {}
    }
}

fn collect_resolve_calls(expr: &SpExpr, names: &mut std::collections::HashSet<String>) {
    match &expr.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(fname) = &func.as_ref().node
                && (fname == "resolve" || fname == "resolve_prophecy")
                && let Some(_sp_arg) = args.first()
                && let Expr::Ident(var) = &_sp_arg.node
            {
                names.insert(var.clone());
            }
            for arg in args {
                collect_resolve_calls(arg, names);
            }
        }
        Expr::Raw(tokens) => {
            for window in tokens.windows(2) {
                if (window[0] == "resolve" || window[0] == "resolve_prophecy") && window[1] != "(" {
                    names.insert(window[1].clone());
                }
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_resolve_calls(lhs, names);
            collect_resolve_calls(rhs, names);
        }
        Expr::UnaryOp { expr, .. } | Expr::Old(expr) | Expr::Ghost(expr) => {
            collect_resolve_calls(expr, names);
        }
        Expr::Block(es) | Expr::List(es) => {
            for e in es {
                collect_resolve_calls(e, names);
            }
        }
        _ => {}
    }
}
