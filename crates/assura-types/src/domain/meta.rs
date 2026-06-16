//! Meta-level domain checkers.
//!
//! ComplexityBoundChecker, BehavioralEquivalenceChecker,
//! MultiPassRefinementChecker, IncrementalContractChecker,
//! ScopedInvariantChecker, ContractCompositionChecker,
//! ContractLibraryChecker.

use crate::TypeError;

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
                        span: 0..1,
                        secondary: None,
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
                        span: 0..1,
                        secondary: None,
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
                        span: 0..1,
                        secondary: None,
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
