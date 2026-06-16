//! Concurrency-related domain checkers.
//!
//! CallbackReentrancyChecker, TemporalDeadlineChecker.

use std::collections::HashMap;
use std::ops::Range;

use crate::TypeError;

// ===========================================================================
// T066: CONC.2 Callback re-entrancy prevention
// ===========================================================================

/// Prevents re-entrant calls through callback chains.
///
/// Error codes:
/// - A24001: re-entrant callback invocation detected
/// - A24002: callback registered in non-reentrant context
/// - A24003: unbounded callback depth
#[derive(Debug, Clone)]
pub(crate) struct CallbackReentrancyChecker {
    /// Functions currently on the call stack
    call_stack: Vec<String>,
    /// Functions marked as non-reentrant
    non_reentrant: HashMap<String, Range<usize>>,
    /// Maximum allowed callback depth
    max_depth: usize,
}

impl CallbackReentrancyChecker {
    pub fn new() -> Self {
        Self {
            call_stack: Vec::new(),
            non_reentrant: HashMap::new(),
            max_depth: 16,
        }
    }

    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    pub fn mark_non_reentrant(&mut self, fn_name: String, span: Range<usize>) {
        self.non_reentrant.insert(fn_name, span);
    }

    pub fn enter_call(&mut self, fn_name: &str, span: &Range<usize>) -> Vec<TypeError> {
        let mut errors = Vec::new();

        // Check re-entrancy
        if self.call_stack.contains(&fn_name.to_string())
            && self.non_reentrant.contains_key(fn_name)
        {
            errors.push(TypeError {
                code: "A24001".into(),
                message: format!("re-entrant call to non-reentrant function `{fn_name}`"),
                span: span.clone(),
                secondary: None,
            });
        }

        // Check depth
        if self.call_stack.len() >= self.max_depth {
            errors.push(TypeError {
                code: "A24003".into(),
                message: format!(
                    "callback depth {} exceeds maximum {}",
                    self.call_stack.len() + 1,
                    self.max_depth
                ),
                span: span.clone(),
                secondary: None,
            });
        }

        self.call_stack.push(fn_name.to_string());
        errors
    }

    pub fn exit_call(&mut self) {
        self.call_stack.pop();
    }

    pub fn check_register_callback(
        &self,
        target_fn: &str,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if self.non_reentrant.contains_key(target_fn)
            && self.call_stack.contains(&target_fn.to_string())
        {
            return Some(TypeError {
                code: "A24002".into(),
                message: format!(
                    "registering callback to non-reentrant `{target_fn}` while inside it"
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn current_depth(&self) -> usize {
        self.call_stack.len()
    }
}

impl Default for CallbackReentrancyChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T069: CONC.5 Temporal deadlines
// ===========================================================================

/// Enforces bounded response time contracts.
///
/// Error codes:
/// - A25001: operation exceeds declared deadline
/// - A25002: nested deadline violation (inner > outer)
/// - A25003: unbounded operation in deadline context
#[derive(Debug, Clone)]
pub(crate) struct TemporalDeadlineChecker {
    /// Active deadline scopes (name -> deadline_ms)
    deadlines: Vec<(String, u64)>,
    /// Operations with known worst-case times
    operation_bounds: HashMap<String, u64>,
}

impl TemporalDeadlineChecker {
    pub fn new() -> Self {
        Self {
            deadlines: Vec::new(),
            operation_bounds: HashMap::new(),
        }
    }

    pub fn register_bound(&mut self, op: String, worst_case_ms: u64) {
        self.operation_bounds.insert(op, worst_case_ms);
    }

    pub fn enter_deadline(
        &mut self,
        name: String,
        deadline_ms: u64,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        // Check nested deadline doesn't exceed outer
        if let Some((outer_name, outer_ms)) = self.deadlines.last()
            && deadline_ms > *outer_ms
        {
            return Some(TypeError {
                code: "A25002".into(),
                message: format!(
                    "inner deadline `{name}` ({deadline_ms}ms) exceeds outer `{outer_name}` ({outer_ms}ms)"
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        self.deadlines.push((name, deadline_ms));
        None
    }

    pub fn exit_deadline(&mut self) {
        self.deadlines.pop();
    }

    pub fn check_operation(&self, op: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some((deadline_name, deadline_ms)) = self.deadlines.last() {
            if let Some(worst_case) = self.operation_bounds.get(op) {
                if worst_case > deadline_ms {
                    return Some(TypeError {
                        code: "A25001".into(),
                        message: format!(
                            "operation `{op}` worst-case {worst_case}ms exceeds deadline `{deadline_name}` ({deadline_ms}ms)"
                        ),
                        span: span.clone(),
                        secondary: None,
                    });
                }
            } else {
                return Some(TypeError {
                    code: "A25003".into(),
                    message: format!(
                        "unbounded operation `{op}` in deadline context `{deadline_name}`"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
        }
        None
    }

    pub fn current_deadline(&self) -> Option<(&str, u64)> {
        self.deadlines.last().map(|(n, d)| (n.as_str(), *d))
    }
}

impl Default for TemporalDeadlineChecker {
    fn default() -> Self {
        Self::new()
    }
}
