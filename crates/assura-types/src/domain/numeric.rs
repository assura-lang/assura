//! Numeric-related domain checkers.
//!
//! NumericalPrecisionChecker, PrecomputedTableChecker.

use crate::TypeError;

// ===========================================================================
// T095: NUM.1 Numerical precision
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct NumericalPrecisionChecker {
    variables: std::collections::HashMap<String, PrecisionInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct PrecisionInfo {
    pub bits: u32,
    pub min_ulp: f64,
    pub span: std::ops::Range<usize>,
}

impl NumericalPrecisionChecker {
    pub fn new() -> Self {
        Self {
            variables: std::collections::HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: String, bits: u32, min_ulp: f64, span: std::ops::Range<usize>) {
        self.variables.insert(
            name,
            PrecisionInfo {
                bits,
                min_ulp,
                span,
            },
        );
    }

    pub fn check_precision_loss(&self, name: &str, result_bits: u32) -> Option<TypeError> {
        if let Some(info) = self.variables.get(name)
            && result_bits < info.bits
        {
            return Some(TypeError {
                code: "A42001".into(),
                message: format!(
                    "precision loss: `{name}` requires {}-bit but operation produces {result_bits}-bit",
                    info.bits
                ),
                span: info.span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_ulp_bound(&self, name: &str, actual_ulp: f64) -> Option<TypeError> {
        if let Some(info) = self.variables.get(name)
            && actual_ulp > info.min_ulp
        {
            return Some(TypeError {
                code: "A42002".into(),
                message: format!(
                    "ULP violation: `{name}` requires ULP <= {} but got {actual_ulp}",
                    info.min_ulp
                ),
                span: info.span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_cancellation(&self, name: &str, operand_ratio: f64) -> Option<TypeError> {
        if operand_ratio > 0.999
            && let Some(info) = self.variables.get(name)
        {
            return Some(TypeError {
                code: "A42003".into(),
                message: format!(
                    "potential catastrophic cancellation in `{name}` (operand ratio: {operand_ratio})"
                ),
                span: info.span.clone(),
                secondary: None,
            });
        }
        None
    }
}

impl Default for NumericalPrecisionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T096: NUM.2 Precomputed table verification
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct PrecomputedTableChecker {
    tables: Vec<TableDecl>,
}

#[derive(Debug, Clone)]
pub(crate) struct TableDecl {
    pub name: String,
    pub size: usize,
    pub verified_entries: usize,
    pub generator_fn: String,
    pub span: std::ops::Range<usize>,
}

impl PrecomputedTableChecker {
    pub fn new() -> Self {
        Self { tables: Vec::new() }
    }

    pub fn declare_table(
        &mut self,
        name: String,
        size: usize,
        generator_fn: String,
        span: std::ops::Range<usize>,
    ) {
        self.tables.push(TableDecl {
            name,
            size,
            verified_entries: 0,
            generator_fn,
            span,
        });
    }

    pub fn mark_entries_verified(&mut self, name: &str, count: usize) {
        if let Some(t) = self.tables.iter_mut().find(|t| t.name == name) {
            t.verified_entries = count;
        }
    }

    pub fn check_coverage(&self) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| t.verified_entries < t.size)
            .map(|t| TypeError {
                code: "A43001".into(),
                message: format!(
                    "table `{}` has only {}/{} entries verified",
                    t.name, t.verified_entries, t.size
                ),
                span: t.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_generator(&self) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| t.generator_fn.is_empty())
            .map(|t| TypeError {
                code: "A43002".into(),
                message: format!("table `{}` has no generator function", t.name),
                span: t.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_non_empty(&self) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| t.size == 0)
            .map(|t| TypeError {
                code: "A43003".into(),
                message: format!("table `{}` has zero size", t.name),
                span: t.span.clone(),
                secondary: None,
            })
            .collect()
    }

    /// Validate that declared generator functions exist in the source.
    pub fn check_generator_exists(&self, fn_names: &[String]) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| !t.generator_fn.is_empty() && !fn_names.contains(&t.generator_fn))
            .map(|t| TypeError {
                code: "A43004".into(),
                message: format!(
                    "table `{}` references generator function `{}` which is not defined",
                    t.name, t.generator_fn
                ),
                span: t.span.clone(),
                secondary: None,
            })
            .collect()
    }
}

impl Default for PrecomputedTableChecker {
    fn default() -> Self {
        Self::new()
    }
}
