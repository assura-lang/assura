//! Meta-level domain checkers.
//!
//! ComplexityBoundChecker, BehavioralEquivalenceChecker,
//! MultiPassRefinementChecker, IncrementalContractChecker,
//! ScopedInvariantChecker, ContractCompositionChecker,
//! ContractLibraryChecker, MatchExhaustivenessChecker.

use assura_parser::ast::{BlockKind, ClauseKind, Decl, Expr, ExprVisitor, MatchArm, SpExpr};

use crate::checkers::{
    InterfaceChecker, InterfaceContract, InterfaceMethod, InvariantKind, StructuralInvariant,
    StructuralInvariantChecker, collect_ident_references, extract_call, extract_ident,
    extract_int_literal, extract_kv_pairs,
};
use crate::checks::{clauses_contract_fn, clauses_contract_fn_block, clauses_contract_fn_extern};
use crate::convert::parse_type_tokens;
use crate::types::*;
use crate::{Type, TypeError};

// ===========================================================================
// T101: PERF.2 Complexity bounds (AARA)
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct ComplexityBoundChecker {
    bounds: std::collections::HashMap<String, ComplexityBound>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComplexityClass {
    Constant,
    Logarithmic,
    Linear,
    NLogN,
    Quadratic,
    Cubic,
    Exponential,
}

#[derive(Debug, Clone)]
pub(crate) struct ComplexityBound {
    pub declared: ComplexityClass,
    pub measured: Option<ComplexityClass>,
    pub span: std::ops::Range<usize>,
}

impl ComplexityBoundChecker {
    pub fn new() -> Self {
        Self {
            bounds: std::collections::HashMap::new(),
        }
    }

    pub fn declare_bound(
        &mut self,
        fn_name: String,
        declared: ComplexityClass,
        span: std::ops::Range<usize>,
    ) {
        self.bounds.insert(
            fn_name,
            ComplexityBound {
                declared,
                measured: None,
                span,
            },
        );
    }

    pub fn record_measured(&mut self, fn_name: &str, measured: ComplexityClass) {
        if let Some(b) = self.bounds.get_mut(fn_name) {
            b.measured = Some(measured);
        }
    }

    fn class_rank(c: &ComplexityClass) -> u8 {
        match c {
            ComplexityClass::Constant => 0,
            ComplexityClass::Logarithmic => 1,
            ComplexityClass::Linear => 2,
            ComplexityClass::NLogN => 3,
            ComplexityClass::Quadratic => 4,
            ComplexityClass::Cubic => 5,
            ComplexityClass::Exponential => 6,
        }
    }

    pub fn check_bounds(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, bound) in &self.bounds {
            if let Some(ref measured) = bound.measured
                && Self::class_rank(measured) > Self::class_rank(&bound.declared)
            {
                errors.push(TypeError {
                    code: "A48001".into(),
                    message: format!(
                        "function `{name}` declared as {:?} but measured as {measured:?}",
                        bound.declared
                    ),
                    span: bound.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_unverified(&self) -> Vec<TypeError> {
        self.bounds
            .iter()
            .filter(|(_, b)| b.measured.is_none())
            .map(|(n, b)| TypeError {
                code: "A48002".into(),
                message: format!("complexity bound for `{n}` is not verified"),
                span: b.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_expensive(&self) -> Vec<TypeError> {
        self.bounds
            .iter()
            .filter(|(_, b)| b.declared == ComplexityClass::Exponential)
            .map(|(n, b)| TypeError {
                code: "A48003".into(),
                message: format!("function `{n}` has exponential complexity bound"),
                span: b.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }
}

impl Default for ComplexityBoundChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T102: TEST.2 Behavioral equivalence
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct BehavioralEquivalenceChecker {
    equivalences: Vec<EquivalenceDecl>,
}

#[derive(Debug, Clone)]
pub(crate) struct EquivalenceDecl {
    pub name: String,
    pub impl_a: String,
    pub impl_b: String,
    pub contract: String,
    pub verified: bool,
    pub span: std::ops::Range<usize>,
}

impl BehavioralEquivalenceChecker {
    pub fn new() -> Self {
        Self {
            equivalences: Vec::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: String,
        impl_a: String,
        impl_b: String,
        contract: String,
        span: std::ops::Range<usize>,
    ) {
        self.equivalences.push(EquivalenceDecl {
            name,
            impl_a,
            impl_b,
            contract,
            verified: false,
            span,
        });
    }

    pub fn mark_verified(&mut self, name: &str) {
        if let Some(e) = self.equivalences.iter_mut().find(|e| e.name == name) {
            e.verified = true;
        }
    }

    pub fn check_unverified(&self) -> Vec<TypeError> {
        self.equivalences
            .iter()
            .filter(|e| !e.verified)
            .map(|e| TypeError {
                code: "A49001".into(),
                message: format!(
                    "behavioral equivalence `{}` between `{}` and `{}` not verified",
                    e.name, e.impl_a, e.impl_b
                ),
                span: e.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_self_equivalence(&self) -> Vec<TypeError> {
        self.equivalences
            .iter()
            .filter(|e| e.impl_a == e.impl_b)
            .map(|e| TypeError {
                code: "A49002".into(),
                message: format!(
                    "trivial self-equivalence in `{}`: both sides are `{}`",
                    e.name, e.impl_a
                ),
                span: e.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_contract_ref(&self) -> Vec<TypeError> {
        self.equivalences
            .iter()
            .filter(|e| e.contract.is_empty())
            .map(|e| TypeError {
                code: "A49003".into(),
                message: format!("equivalence `{}` has no contract reference", e.name),
                span: e.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }
}

impl Default for BehavioralEquivalenceChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T103: TEST.3 Multi-pass refinement
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct MultiPassRefinementChecker {
    passes: Vec<RefinementPass>,
}

#[derive(Debug, Clone)]
pub(crate) struct RefinementPass {
    pub name: String,
    pub from_level: String,
    pub to_level: String,
    pub obligations_total: usize,
    pub obligations_discharged: usize,
    pub span: std::ops::Range<usize>,
}

impl MultiPassRefinementChecker {
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    pub fn add_pass(
        &mut self,
        name: String,
        from_level: String,
        to_level: String,
        obligations: usize,
        span: std::ops::Range<usize>,
    ) {
        self.passes.push(RefinementPass {
            name,
            from_level,
            to_level,
            obligations_total: obligations,
            obligations_discharged: 0,
            span,
        });
    }

    pub fn discharge(&mut self, pass_name: &str, count: usize) {
        if let Some(p) = self.passes.iter_mut().find(|p| p.name == pass_name) {
            p.obligations_discharged += count;
        }
    }

    pub fn check_complete(&self) -> Vec<TypeError> {
        self.passes
            .iter()
            .filter(|p| p.obligations_discharged < p.obligations_total)
            .map(|p| TypeError {
                code: "A50001".into(),
                message: format!(
                    "refinement `{}` ({} -> {}): {}/{} obligations discharged",
                    p.name, p.from_level, p.to_level, p.obligations_discharged, p.obligations_total
                ),
                span: p.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_chain(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for i in 1..self.passes.len() {
            if self.passes[i].from_level != self.passes[i - 1].to_level {
                errors.push(TypeError {
                    code: "A50002".into(),
                    message: format!(
                        "refinement chain gap: `{}` starts at `{}` but `{}` ends at `{}`",
                        self.passes[i].name,
                        self.passes[i].from_level,
                        self.passes[i - 1].name,
                        self.passes[i - 1].to_level
                    ),
                    span: self.passes[i].span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_non_trivial(&self) -> Vec<TypeError> {
        self.passes
            .iter()
            .filter(|p| p.obligations_total == 0)
            .map(|p| TypeError {
                code: "A50003".into(),
                message: format!("refinement pass `{}` has zero obligations", p.name),
                span: p.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }
}

impl Default for MultiPassRefinementChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T104: MISC.1 Incremental contracts
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct IncrementalContractChecker {
    contracts: std::collections::HashMap<String, ContractHistoryEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContractHistoryEntry {
    pub versions: Vec<ContractVersionEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContractVersionEntry {
    pub version: u32,
    pub requires_count: usize,
    pub ensures_count: usize,
    pub span: std::ops::Range<usize>,
}

impl IncrementalContractChecker {
    pub fn new() -> Self {
        Self {
            contracts: std::collections::HashMap::new(),
        }
    }

    pub fn add_version(
        &mut self,
        name: String,
        version: u32,
        requires_count: usize,
        ensures_count: usize,
        span: std::ops::Range<usize>,
    ) {
        let history = self
            .contracts
            .entry(name)
            .or_insert_with(|| ContractHistoryEntry {
                versions: Vec::new(),
            });
        history.versions.push(ContractVersionEntry {
            version,
            requires_count,
            ensures_count,
            span,
        });
    }

    pub fn check_precondition_weakening(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, history) in &self.contracts {
            for i in 1..history.versions.len() {
                if history.versions[i].requires_count > history.versions[i - 1].requires_count {
                    errors.push(TypeError {
                        code: "A51001".into(),
                        message: format!(
                            "contract `{name}` v{} strengthens preconditions",
                            history.versions[i].version
                        ),
                        span: history.versions[i].span.clone(),
                        secondary: Some((
                            history.versions[i - 1].span.clone(),
                            format!("previous version v{}", history.versions[i - 1].version),
                        )),
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_postcondition_strengthening(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, history) in &self.contracts {
            for i in 1..history.versions.len() {
                if history.versions[i].ensures_count < history.versions[i - 1].ensures_count {
                    errors.push(TypeError {
                        code: "A51002".into(),
                        message: format!(
                            "contract `{name}` v{} weakens postconditions",
                            history.versions[i].version
                        ),
                        span: history.versions[i].span.clone(),
                        secondary: Some((
                            history.versions[i - 1].span.clone(),
                            format!("previous version v{}", history.versions[i - 1].version),
                        )),
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_version_continuity(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, history) in &self.contracts {
            for i in 1..history.versions.len() {
                if history.versions[i].version != history.versions[i - 1].version + 1 {
                    errors.push(TypeError {
                        code: "A51003".into(),
                        message: format!(
                            "contract `{name}` has version gap: v{} to v{}",
                            history.versions[i - 1].version,
                            history.versions[i].version
                        ),
                        span: history.versions[i].span.clone(),
                        secondary: Some((
                            history.versions[i - 1].span.clone(),
                            format!("v{}", history.versions[i - 1].version),
                        )),
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }
}

impl Default for IncrementalContractChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T105: MISC.2 Scoped invariant suspension
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct ScopedInvariantChecker {
    invariants: std::collections::HashMap<String, InvariantState>,
    suspension_depth: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InvariantState {
    Active,
    Suspended,
    Restored,
}

impl ScopedInvariantChecker {
    pub fn new() -> Self {
        Self {
            invariants: std::collections::HashMap::new(),
            suspension_depth: 0,
        }
    }

    pub fn declare_invariant(&mut self, name: String) {
        self.invariants.insert(name, InvariantState::Active);
    }

    pub fn suspend(&mut self, name: &str) -> Option<TypeError> {
        if let Some(state) = self.invariants.get_mut(name) {
            if *state == InvariantState::Suspended {
                return Some(TypeError {
                    code: "A52001".into(),
                    message: format!("invariant `{name}` is already suspended"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
            *state = InvariantState::Suspended;
            self.suspension_depth += 1;
            None
        } else {
            Some(TypeError {
                code: "A52002".into(),
                message: format!("cannot suspend undeclared invariant `{name}`"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
        }
    }

    pub fn restore(&mut self, name: &str) -> Option<TypeError> {
        if let Some(state) = self.invariants.get_mut(name) {
            if *state != InvariantState::Suspended {
                return Some(TypeError {
                    code: "A52003".into(),
                    message: format!("invariant `{name}` is not currently suspended"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
            *state = InvariantState::Restored;
            if self.suspension_depth > 0 {
                self.suspension_depth -= 1;
            }
            None
        } else {
            None
        }
    }

    pub fn check_all_restored(&self) -> Vec<TypeError> {
        self.invariants
            .iter()
            .filter(|(_, s)| **s == InvariantState::Suspended)
            .map(|(n, _)| TypeError {
                code: "A52001".into(),
                message: format!("invariant `{n}` still suspended at scope exit"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn is_suspended(&self, name: &str) -> bool {
        self.invariants.get(name) == Some(&InvariantState::Suspended)
    }
}

impl Default for ScopedInvariantChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T110: Contract composition with extends
// ===========================================================================

/// Tracks contract inheritance/composition via extends.
#[derive(Debug, Clone)]
pub(crate) struct ContractCompositionChecker {
    contracts: std::collections::HashMap<String, ComposableContract>,
}

#[derive(Debug, Clone)]
pub(crate) struct ComposableContract {
    pub name: String,
    pub extends: Vec<String>,
    pub own_clauses: usize,
}

impl ContractCompositionChecker {
    pub fn new() -> Self {
        Self {
            contracts: std::collections::HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: String, extends: Vec<String>, own_clauses: usize) {
        self.contracts.insert(
            name.clone(),
            ComposableContract {
                name,
                extends,
                own_clauses,
            },
        );
    }

    /// Check that all extended contracts exist.
    pub fn check_extends(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, contract) in &self.contracts {
            for parent in &contract.extends {
                if !self.contracts.contains_key(parent) {
                    errors.push(TypeError {
                        code: "A54001".into(),
                        message: format!("contract `{name}` extends unknown contract `{parent}`"),
                        span: 0..1,
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    /// Check for circular extends.
    pub fn check_circular(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for name in self.contracts.keys() {
            let mut visited = vec![name.clone()];
            if self.has_extends_cycle(name, &mut visited) {
                errors.push(TypeError {
                    code: "A54002".into(),
                    message: format!("circular extends chain involving `{name}`"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    fn has_extends_cycle(&self, current: &str, visited: &mut Vec<String>) -> bool {
        if let Some(contract) = self.contracts.get(current) {
            for parent in &contract.extends {
                if visited.contains(parent) {
                    return true;
                }
                visited.push(parent.clone());
                if self.has_extends_cycle(parent, visited) {
                    return true;
                }
                visited.pop();
            }
        }
        false
    }

    /// Check for diamond inheritance (same contract extended via two paths).
    pub fn check_diamond(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, contract) in &self.contracts {
            let mut all_ancestors = Vec::new();
            for parent in &contract.extends {
                let ancestors = self.collect_ancestors(parent);
                for a in &ancestors {
                    if all_ancestors.contains(a) {
                        errors.push(TypeError {
                            code: "A54003".into(),
                            message: format!(
                                "diamond inheritance in `{name}`: `{a}` reached via multiple paths"
                            ),
                            span: 0..1,
                            secondary: None,
                            suggestion: None,
                        });
                    }
                }
                all_ancestors.extend(ancestors);
            }
        }
        errors
    }

    fn collect_ancestors(&self, name: &str) -> Vec<String> {
        let mut result = vec![name.to_string()];
        if let Some(c) = self.contracts.get(name) {
            for parent in &c.extends {
                result.extend(self.collect_ancestors(parent));
            }
        }
        result
    }

    /// Check for contracts with zero own clauses (pure composition).
    pub fn check_empty_contracts(&self) -> Vec<TypeError> {
        self.contracts
            .values()
            .filter(|c| c.own_clauses == 0 && c.extends.is_empty())
            .map(|c| TypeError {
                code: "A54003".into(),
                message: format!("contract `{}` has no clauses and extends nothing", c.name),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }
}

impl Default for ContractCompositionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T111: Contract libraries as publishable packages
// ===========================================================================

/// Tracks contract library packaging metadata.
#[derive(Debug, Clone)]
pub(crate) struct ContractLibraryChecker {
    libraries: Vec<ContractLibrary>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContractLibrary {
    pub name: String,
    pub version: String,
    pub exported_contracts: Vec<String>,
    pub dependencies: Vec<LibraryDep>,
}

#[derive(Debug, Clone)]
pub(crate) struct LibraryDep {
    pub name: String,
    pub version_req: String,
}

impl ContractLibraryChecker {
    pub fn new() -> Self {
        Self {
            libraries: Vec::new(),
        }
    }

    pub fn declare_library(&mut self, name: String, version: String) {
        self.libraries.push(ContractLibrary {
            name,
            version,
            exported_contracts: Vec::new(),
            dependencies: Vec::new(),
        });
    }

    pub fn add_export(&mut self, lib_name: &str, contract: String) {
        if let Some(lib) = self.libraries.iter_mut().find(|l| l.name == lib_name) {
            lib.exported_contracts.push(contract);
        }
    }

    pub fn add_dependency(&mut self, lib_name: &str, dep: LibraryDep) {
        if let Some(lib) = self.libraries.iter_mut().find(|l| l.name == lib_name) {
            lib.dependencies.push(dep);
        }
    }

    /// Check for libraries with no exports.
    pub fn check_empty_exports(&self) -> Vec<TypeError> {
        self.libraries
            .iter()
            .filter(|l| l.exported_contracts.is_empty())
            .map(|l| TypeError {
                code: "A55001".into(),
                message: format!("library `{}` has no exported contracts", l.name),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    /// Check for circular dependencies.
    pub fn check_circular_deps(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for lib in &self.libraries {
            for dep in &lib.dependencies {
                if dep.name == lib.name {
                    errors.push(TypeError {
                        code: "A55002".into(),
                        message: format!("library `{}` depends on itself", lib.name),
                        span: 0..1,
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    /// Check for duplicate library names.
    pub fn check_duplicates(&self) -> Vec<TypeError> {
        let mut seen = std::collections::HashSet::new();
        let mut errors = Vec::new();
        for lib in &self.libraries {
            if !seen.insert(lib.name.clone()) {
                errors.push(TypeError {
                    code: "A55003".into(),
                    message: format!("duplicate library name `{}`", lib.name),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    /// Check for version constraint compatibility between libraries and deps.
    pub fn check_version_compat(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for lib in &self.libraries {
            for dep in &lib.dependencies {
                if dep.version_req != "*" && dep.version_req != lib.version {
                    // Check if any declared library matches the dep
                    let dep_lib = self.libraries.iter().find(|l| l.name == dep.name);
                    if let Some(found) = dep_lib
                        && dep.version_req != found.version
                    {
                        errors.push(TypeError {
                            code: "A55003".into(),
                            message: format!(
                                "library `{}` v{} depends on `{}` v{} but found v{}",
                                lib.name, lib.version, dep.name, dep.version_req, found.version
                            ),
                            span: 0..1,
                            secondary: None,
                            suggestion: None,
                        });
                    }
                }
            }
        }
        errors
    }
}

impl Default for ContractLibraryChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Pattern exhaustiveness (T017)
// ===========================================================================

/// A pattern in a match arm, used for exhaustiveness checking.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Pattern {
    /// Matches a specific enum variant by name.
    Variant(String),
    /// Wildcard `_` pattern that matches anything.
    Wildcard,
    /// Matches a specific literal value.
    Literal(assura_parser::ast::Literal),
}

/// Check whether a set of patterns exhaustively covers all variants of an enum.
///
/// Returns `None` if the patterns are exhaustive, or `Some(missing)` with the
/// list of uncovered variant names.
pub(crate) fn check_exhaustiveness(
    patterns: &[Pattern],
    enum_variants: &[String],
) -> Option<Vec<String>> {
    if patterns.iter().any(|p| matches!(p, Pattern::Wildcard)) {
        return None;
    }
    let covered: std::collections::HashSet<&str> = patterns
        .iter()
        .filter_map(|p| match p {
            Pattern::Variant(name) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let missing: Vec<String> = enum_variants
        .iter()
        .filter(|v| !covered.contains(v.as_str()))
        .cloned()
        .collect();
    if missing.is_empty() {
        None
    } else {
        Some(missing)
    }
}

// ===========================================================================
// Match exhaustiveness source walking (T017)
// ===========================================================================

/// Walk all expressions in the source file and check match expressions
/// for exhaustiveness against known enum types.
pub(crate) fn run_match_exhaustiveness_source(
    source: &assura_parser::ast::SourceFile,
    symbols: &assura_resolve::SymbolTable,
) -> Vec<TypeError> {
    let mut errors = Vec::new();
    let mut enum_variants: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for decl in &source.decls {
        if let Decl::EnumDef(e) = &decl.node {
            enum_variants.insert(
                e.name.clone(),
                e.variants.iter().map(|v| v.name.clone()).collect(),
            );
        }
    }
    for decl in &source.decls {
        let Some(clauses) = clauses_contract_fn_extern(&decl.node) else {
            continue;
        };
        for clause in clauses {
            check_match_exhaustiveness_expr(
                &clause.body,
                &decl.span,
                &enum_variants,
                symbols,
                &mut errors,
            );
        }
    }
    errors
}

fn check_match_exhaustiveness_expr(
    expr: &SpExpr,
    span: &std::ops::Range<usize>,
    enum_variants: &std::collections::HashMap<String, Vec<String>>,
    _symbols: &assura_resolve::SymbolTable,
    errors: &mut Vec<TypeError>,
) {
    struct MatchExhaustivenessVisitor<'a> {
        span: &'a std::ops::Range<usize>,
        enum_variants: &'a std::collections::HashMap<String, Vec<String>>,
        errors: &'a mut Vec<TypeError>,
    }

    impl ExprVisitor for MatchExhaustivenessVisitor<'_> {
        fn visit_match(&mut self, scrutinee: &SpExpr, arms: &[MatchArm]) {
            self.visit_expr(scrutinee);
            for arm in arms {
                self.visit_expr(&arm.body);
            }
            if let Expr::Ident(name) = &scrutinee.node
                && let Some(variants) = self.enum_variants.get(name)
            {
                let patterns: Vec<Pattern> = arms
                    .iter()
                    .map(|arm| match &arm.pattern {
                        assura_parser::ast::Pattern::Ident(n) => Pattern::Variant(n.clone()),
                        assura_parser::ast::Pattern::Wildcard => Pattern::Wildcard,
                        assura_parser::ast::Pattern::Literal(lit) => Pattern::Literal(lit.clone()),
                        assura_parser::ast::Pattern::Constructor { name, .. } => {
                            Pattern::Variant(name.clone())
                        }
                        assura_parser::ast::Pattern::Tuple(_) => Pattern::Wildcard,
                    })
                    .collect();

                if let Some(missing) = check_exhaustiveness(&patterns, variants) {
                    self.errors.push(TypeError {
                        code: "A10001".into(),
                        message: format!(
                            "non-exhaustive match: missing variants {}",
                            missing.join(", ")
                        ),
                        span: self.span.clone(),
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
            let has_wildcard = arms
                .iter()
                .any(|arm| matches!(arm.pattern, assura_parser::ast::Pattern::Wildcard));
            let has_enum_coverage = if let Expr::Ident(name) = &scrutinee.node {
                self.enum_variants.contains_key(name)
            } else {
                false
            };
            if !has_wildcard && !has_enum_coverage && !arms.is_empty() {
                self.errors.push(TypeError {
                    code: "A10002".into(),
                    message: "match expression on unknown type has no wildcard `_` arm; \
                              consider adding a catch-all pattern"
                        .into(),
                    span: self.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
    }

    let mut visitor = MatchExhaustivenessVisitor {
        span,
        enum_variants,
        errors,
    };
    visitor.visit_expr(expr);
}

// ===========================================================================
// Interface contracts source walking (T062)
// ===========================================================================

fn extract_interface_method(body: &SpExpr) -> Option<InterfaceMethod> {
    match &body.node {
        Expr::Ident(name) => Some(InterfaceMethod {
            name: name.clone(),
            param_types: vec![],
            return_type: Type::Unknown,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }),
        Expr::Call { func, args } => {
            let name = match &func.as_ref().node {
                Expr::Ident(n) => n.clone(),
                _ => return None,
            };
            let param_types: Vec<Type> = args
                .iter()
                .map(|arg| match &arg.node {
                    Expr::Ident(t) => parse_type_tokens(std::slice::from_ref(t)),
                    _ => Type::Unknown,
                })
                .collect();
            Some(InterfaceMethod {
                name,
                param_types,
                return_type: Type::Unknown,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: false,
            })
        }
        Expr::Raw(tokens) => {
            let name = tokens.first()?.clone();
            let mut param_types = Vec::new();
            let mut return_type = Type::Unknown;
            if let Some(paren_start) = tokens.iter().position(|t| t == "(")
                && let Some(paren_end) = tokens.iter().position(|t| t == ")")
            {
                let param_tokens = &tokens[paren_start + 1..paren_end];
                for chunk in param_tokens.split(|t| t == ",") {
                    if !chunk.is_empty() {
                        let owned: Vec<String> = chunk.to_vec();
                        param_types.push(parse_type_tokens(&owned));
                    }
                }
                if let Some(arrow_pos) = tokens[paren_end..].iter().position(|t| t == "->") {
                    let ret_tokens: Vec<String> = tokens[paren_end + arrow_pos + 1..].to_vec();
                    if !ret_tokens.is_empty() {
                        return_type = parse_type_tokens(&ret_tokens);
                    }
                }
            }
            Some(InterfaceMethod {
                name,
                param_types,
                return_type,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: false,
            })
        }
        _ => None,
    }
}

impl InterfaceChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = InterfaceChecker::new();
        let mut errors = Vec::new();

        for decl in &source.decls {
            if let Decl::Contract(c) = &decl.node {
                let is_interface = c
                    .clauses
                    .iter()
                    .any(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "interface"));
                if is_interface {
                    let methods: Vec<InterfaceMethod> = c
                        .clauses
                        .iter()
                        .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "method"))
                        .filter_map(|cl| extract_interface_method(&cl.body))
                        .collect();
                    let extends: Vec<String> = c
                        .clauses
                        .iter()
                        .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "extends"))
                        .filter_map(|cl| {
                            if let Expr::Ident(name) = &cl.body.node {
                                Some(name.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                    checker.register_interface(InterfaceContract {
                        name: c.name.clone(),
                        methods,
                        extends,
                    });
                }
            }
        }

        for decl in &source.decls {
            if let Decl::Contract(c) = &decl.node {
                for clause in &c.clauses {
                    if let ClauseKind::Other(k) = &clause.kind
                        && k == "implements"
                        && let Expr::Ident(iface_name) = &clause.body.node
                    {
                        let impl_methods: Vec<InterfaceMethod> = c
                            .clauses
                            .iter()
                            .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "method"))
                            .filter_map(|cl| extract_interface_method(&cl.body))
                            .collect();
                        let method_names: Vec<String> =
                            impl_methods.iter().map(|m| m.name.clone()).collect();
                        checker.register_impl(
                            c.name.clone(),
                            iface_name.clone(),
                            method_names.clone(),
                        );
                        for err in
                            checker.check_impl(&c.name, iface_name, &method_names, &decl.span)
                        {
                            errors.push(err.into());
                        }
                        for method in &impl_methods {
                            for err in checker.check_method_signature(
                                iface_name,
                                &method.name,
                                &method.param_types,
                                &method.return_type,
                                &decl.span,
                            ) {
                                errors.push(err.into());
                            }
                            let is_reentrant = c.clauses.iter().any(|cl| {
                                matches!(&cl.kind, ClauseKind::Other(k) if k == "reentrant")
                                    && matches!(&cl.body.node, Expr::Ident(n) if n == &method.name)
                            });
                            for err in checker.check_reentrancy(
                                iface_name,
                                &method.name,
                                is_reentrant,
                                &decl.span,
                            ) {
                                errors.push(err.into());
                            }
                        }
                    }
                }
            }
        }

        errors
    }
}

// ===========================================================================
// Structural invariants source walking (T063)
// ===========================================================================

impl StructuralInvariantChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = StructuralInvariantChecker::new();
        let mut errors = Vec::new();

        for decl in &source.decls {
            if let Decl::TypeDef(td) = &decl.node {
                if let assura_parser::ast::TypeBody::Struct(fields) = &td.body {
                    let recursive_fields: Vec<String> = fields
                        .iter()
                        .filter(|f| {
                            let tokens = f.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
                            tokens.iter().any(|t| t == &td.name)
                        })
                        .map(|f| f.name.clone())
                        .collect();
                    if !recursive_fields.is_empty() {
                        checker.register_recursive_type(td.name.clone(), recursive_fields);
                    }
                }
            } else if let Decl::Contract(c) = &decl.node {
                for clause in &c.clauses {
                    if let ClauseKind::Other(k) = &clause.kind
                        && k == "structural_invariant"
                    {
                        let kind = match &clause.body.node {
                            Expr::Ident(name) => match name.as_str() {
                                "sorted" => InvariantKind::Sorted { descending: false },
                                "acyclic" => InvariantKind::Acyclic,
                                "bst_ordering" => InvariantKind::BstOrdering,
                                other => InvariantKind::Custom(other.to_string()),
                            },
                            Expr::Call { func, .. } => {
                                if let Expr::Ident(name) = &func.as_ref().node {
                                    match name.as_str() {
                                        "tree_balance" => {
                                            InvariantKind::TreeBalance { max_diff: 1 }
                                        }
                                        "min_heap" => {
                                            InvariantKind::HeapProperty { min_heap: true }
                                        }
                                        "max_heap" => {
                                            InvariantKind::HeapProperty { min_heap: false }
                                        }
                                        other => InvariantKind::Custom(other.to_string()),
                                    }
                                } else {
                                    InvariantKind::Custom(format!("{:?}", clause.body))
                                }
                            }
                            _ => InvariantKind::Custom(format!("{:?}", clause.body)),
                        };
                        checker.register_invariant(StructuralInvariant {
                            name: format!("{}_{}", c.name, kind),
                            type_name: c.name.clone(),
                            kind: kind.clone(),
                        });
                        for err in checker.check_invariant_applicability(&c.name, &kind, &decl.span)
                        {
                            errors.push(err.into());
                        }
                    }
                    if let ClauseKind::Other(k) = &clause.kind
                        && k == "modifies_structure"
                    {
                        let op_name = match &clause.body.node {
                            Expr::Ident(name) => name.as_str(),
                            _ => "unknown",
                        };
                        let has_preservation = c.clauses.iter().any(|cl| {
                            matches!(&cl.kind, ClauseKind::Other(k2) if k2 == "preserves_invariant")
                        });
                        for err in checker.check_operation_preserves(
                            &c.name,
                            op_name,
                            true,
                            has_preservation,
                            &decl.span,
                        ) {
                            errors.push(err.into());
                        }
                    }
                }
            }
        }

        errors
    }
}

// ===========================================================================
// Complexity bounds source walking
// ===========================================================================

impl ComplexityBoundChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = ComplexityBoundChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn(&decl.node) else {
                continue;
            };
            let Some(name) = decl.node.name().map(|s| s.to_string()) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "complexity" || k == "time_complexity" || k == "big_o")
                {
                    found = true;
                    if let Expr::Ident(class_name) = &clause.body.node {
                        let class = parse_complexity_class(class_name);
                        checker.declare_bound(name.clone(), class, decl.span.clone());
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn(&decl.node) else {
                continue;
            };
            let Some(name) = decl.node.name() else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "measured_complexity" || k == "actual_complexity")
                    && let Expr::Ident(class_name) = &clause.body.node
                {
                    checker.record_measured(name, parse_complexity_class(class_name));
                }
            }
        }
        let mut errors = checker.check_bounds();
        errors.extend(checker.check_unverified());
        errors.extend(checker.check_expensive());
        errors
    }
}

fn parse_complexity_class(name: &str) -> ComplexityClass {
    match name {
        "constant" | "O1" => ComplexityClass::Constant,
        "logarithmic" | "O_log_n" => ComplexityClass::Logarithmic,
        "linear" | "On" => ComplexityClass::Linear,
        "nlogn" | "O_n_log_n" => ComplexityClass::NLogN,
        "quadratic" | "On2" => ComplexityClass::Quadratic,
        "cubic" | "On3" => ComplexityClass::Cubic,
        "exponential" | "O2n" => ComplexityClass::Exponential,
        _ => ComplexityClass::Linear,
    }
}

// ===========================================================================
// Behavioral equivalence source walking
// ===========================================================================

impl BehavioralEquivalenceChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = BehavioralEquivalenceChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            let parent_name = decl.node.name().unwrap_or("");
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "equivalent" || k == "behavioral_equiv" || k == "equiv")
                {
                    found = true;
                    if let Expr::BinOp { lhs, rhs, .. } = &clause.body.node
                        && let (Expr::Ident(a), Expr::Ident(b)) =
                            (&lhs.as_ref().node, &rhs.as_ref().node)
                    {
                        checker.declare(
                            format!("{a}_equiv_{b}"),
                            a.clone(),
                            b.clone(),
                            parent_name.to_string(),
                            decl.span.clone(),
                        );
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "verified_equiv" || k == "equiv_proved")
                    && let Expr::Ident(name) = &clause.body.node
                {
                    checker.mark_verified(name);
                }
            }
        }
        let mut errors = checker.check_unverified();
        errors.extend(checker.check_self_equivalence());
        errors.extend(checker.check_contract_ref());
        errors
    }
}

// ===========================================================================
// Multi-pass refinement source walking
// ===========================================================================

impl MultiPassRefinementChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = MultiPassRefinementChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "refinement_pass" || k == "multi_pass" || k == "refine")
                {
                    found = true;
                    match &clause.body.node {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = &func.as_ref().node {
                                let from = args
                                    .first()
                                    .and_then(extract_ident)
                                    .unwrap_or("abstract")
                                    .to_string();
                                let to = args
                                    .get(1)
                                    .and_then(extract_ident)
                                    .unwrap_or("concrete")
                                    .to_string();
                                let order = args
                                    .get(2)
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_PARAM_ONE)
                                    as usize;
                                checker.add_pass(name.clone(), from, to, order, decl.span.clone());
                            }
                        }
                        Expr::Ident(name) => {
                            checker.add_pass(
                                name.clone(),
                                "abstract".into(),
                                "concrete".into(),
                                1,
                                decl.span.clone(),
                            );
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name" || *k == "pass")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed")
                                .to_string();
                            let from = kvs
                                .iter()
                                .find(|(k, _)| *k == "from" || *k == "source")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("abstract")
                                .to_string();
                            let to = kvs
                                .iter()
                                .find(|(k, _)| *k == "to" || *k == "target")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("concrete")
                                .to_string();
                            let order = kvs
                                .iter()
                                .find(|(k, _)| *k == "order")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ONE)
                                as usize;
                            checker.add_pass(name, from, to, order, decl.span.clone());
                        }
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "discharge_pass" || k == "pass_proved")
                {
                    if let Some((name, args)) = extract_call(&clause.body) {
                        let count =
                            args.first()
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_PARAM_ONE) as usize;
                        checker.discharge(name, count);
                    } else if let Expr::Ident(name) = &clause.body.node {
                        checker.discharge(name, 1);
                    } else {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "pass")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed");
                        let count =
                            kvs.iter()
                                .find(|(k, _)| *k == "count" || *k == "obligations")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ONE) as usize;
                        checker.discharge(name, count);
                    }
                }
            }
        }
        let mut errors = checker.check_complete();
        errors.extend(checker.check_chain());
        errors.extend(checker.check_non_trivial());
        errors
    }
}

// ===========================================================================
// Incremental contracts source walking
// ===========================================================================

impl IncrementalContractChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = IncrementalContractChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            let requires_count = clauses
                .iter()
                .filter(|c| matches!(c.kind, ClauseKind::Requires))
                .count();
            let ensures_count = clauses
                .iter()
                .filter(|c| matches!(c.kind, ClauseKind::Ensures))
                .count();
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "version" || k == "incremental" || k == "contract_version")
                {
                    found = true;
                    match &clause.body.node {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = &func.as_ref().node {
                                let major = args
                                    .first()
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_PARAM_ONE)
                                    as u32;
                                let minor = args
                                    .get(1)
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_PARAM_ZERO)
                                    as u32;
                                let patch = args
                                    .get(2)
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_PARAM_ZERO)
                                    as u32;
                                let version = major * 10000 + minor * 100 + patch;
                                checker.add_version(
                                    name.clone(),
                                    version,
                                    requires_count,
                                    ensures_count,
                                    decl.span.clone(),
                                );
                            }
                        }
                        Expr::Ident(name) => {
                            checker.add_version(
                                name.clone(),
                                10000,
                                requires_count,
                                ensures_count,
                                decl.span.clone(),
                            );
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name" || *k == "contract")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed")
                                .to_string();
                            let major = kvs
                                .iter()
                                .find(|(k, _)| *k == "major")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ONE)
                                as u32;
                            let minor = kvs
                                .iter()
                                .find(|(k, _)| *k == "minor")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ZERO)
                                as u32;
                            let patch = kvs
                                .iter()
                                .find(|(k, _)| *k == "patch")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ZERO)
                                as u32;
                            let version = major * 10000 + minor * 100 + patch;
                            checker.add_version(
                                name,
                                version,
                                requires_count,
                                ensures_count,
                                decl.span.clone(),
                            );
                        }
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = checker.check_precondition_weakening();
        errors.extend(checker.check_postcondition_strengthening());
        errors.extend(checker.check_version_continuity());
        errors
    }
}

// ===========================================================================
// Scoped invariants source walking
// ===========================================================================

impl ScopedInvariantChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = ScopedInvariantChecker::new();
        let mut errors = Vec::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "suspend_invariant" || k == "scoped_invariant" {
                        found = true;
                        if let Expr::Ident(name) = &clause.body.node {
                            checker.declare_invariant(name.clone());
                            if let Some(err) = checker.suspend(name) {
                                errors.push(err);
                            }
                        }
                    }
                    if (k == "restore_invariant" || k == "restore")
                        && let Expr::Ident(name) = &clause.body.node
                        && let Some(err) = checker.restore(name)
                    {
                        errors.push(err);
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        if checker.is_suspended(name) {
                            errors.push(TypeError {
                                code: "A52001".into(),
                                message: format!(
                                    "invariant `{name}` is suspended in active clause context"
                                ),
                                span: decl.span.clone(),
                                secondary: None,
                                suggestion: None,
                            });
                        }
                    }
                }
            }
        }
        errors.extend(checker.check_all_restored());
        errors
    }
}

// ===========================================================================
// Contract composition source walking
// ===========================================================================

impl ContractCompositionChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = ContractCompositionChecker::new();
        let mut found = false;
        for decl in &source.decls {
            if let Decl::Contract(c) = &decl.node {
                let extends: Vec<String> = c
                    .clauses
                    .iter()
                    .filter(|cl| {
                        matches!(&cl.kind, ClauseKind::Other(k) if k == "extends" || k == "inherits")
                    })
                    .filter_map(|cl| {
                        if let Expr::Ident(name) = &cl.body.node {
                            Some(name.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                if !extends.is_empty() {
                    found = true;
                }
                checker.declare(c.name.clone(), extends, c.clauses.len());
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = checker.check_extends();
        errors.extend(checker.check_circular());
        errors.extend(checker.check_diamond());
        errors.extend(checker.check_empty_contracts());
        errors
    }
}

// ===========================================================================
// Contract library source walking
// ===========================================================================

impl ContractLibraryChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = ContractLibraryChecker::new();
        let mut found = false;
        for decl in &source.decls {
            if let Decl::Block {
                kind, name, body, ..
            } = &decl.node
                && *kind == BlockKind::Library
            {
                found = true;
                checker.declare_library(name.clone(), "0.1.0".into());
                for clause in body {
                    if let ClauseKind::Other(ref k) = clause.kind {
                        if (k == "export" || k == "exports")
                            && let Expr::Ident(contract_name) = &clause.body.node
                        {
                            checker.add_export(name, contract_name.clone());
                        }
                        if (k == "depends" || k == "dependency")
                            && let Expr::Ident(dep_name) = &clause.body.node
                        {
                            checker.add_dependency(
                                name,
                                LibraryDep {
                                    name: dep_name.clone(),
                                    version_req: "*".into(),
                                },
                            );
                        }
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = checker.check_empty_exports();
        errors.extend(checker.check_circular_deps());
        errors.extend(checker.check_duplicates());
        errors.extend(checker.check_version_compat());
        errors
    }
}
