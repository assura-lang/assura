//! T079: CORE.6 Opaque functions.

use std::collections::HashMap;
use std::ops::Range;

use crate::TypeError;

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
