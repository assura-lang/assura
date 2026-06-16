//! Safety-related domain checkers.
//!
//! UnsafeEscapeChecker.

use crate::TypeError;

// ===========================================================================
// T100: PERF.1 Unsafe escape with proof
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct UnsafeEscapeChecker {
    unsafe_blocks: Vec<UnsafeBlock>,
}

#[derive(Debug, Clone)]
pub(crate) struct UnsafeBlock {
    pub name: String,
    pub has_safety_proof: bool,
    pub proof_obligations: Vec<String>,
    pub obligations_discharged: Vec<String>,
    pub span: std::ops::Range<usize>,
}

impl UnsafeEscapeChecker {
    pub fn new() -> Self {
        Self {
            unsafe_blocks: Vec::new(),
        }
    }

    pub fn declare_unsafe(
        &mut self,
        name: String,
        obligations: Vec<String>,
        span: std::ops::Range<usize>,
    ) {
        self.unsafe_blocks.push(UnsafeBlock {
            name,
            has_safety_proof: false,
            proof_obligations: obligations,
            obligations_discharged: Vec::new(),
            span,
        });
    }

    pub fn attach_proof(&mut self, name: &str) {
        if let Some(b) = self.unsafe_blocks.iter_mut().find(|b| b.name == name) {
            b.has_safety_proof = true;
        }
    }

    pub fn discharge_obligation(&mut self, block_name: &str, obligation: String) {
        if let Some(b) = self.unsafe_blocks.iter_mut().find(|b| b.name == block_name) {
            b.obligations_discharged.push(obligation);
        }
    }

    pub fn check_unproven(&self) -> Vec<TypeError> {
        self.unsafe_blocks
            .iter()
            .filter(|b| !b.has_safety_proof)
            .map(|b| TypeError {
                code: "A47001".into(),
                message: format!("unsafe block `{}` has no safety proof", b.name),
                span: b.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_obligations(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for b in &self.unsafe_blocks {
            for obl in &b.proof_obligations {
                if !b.obligations_discharged.contains(obl) {
                    errors.push(TypeError {
                        code: "A47002".into(),
                        message: format!(
                            "obligation `{obl}` in unsafe block `{}` not discharged",
                            b.name
                        ),
                        span: b.span.clone(),
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_empty_obligations(&self) -> Vec<TypeError> {
        self.unsafe_blocks
            .iter()
            .filter(|b| b.proof_obligations.is_empty())
            .map(|b| TypeError {
                code: "A47003".into(),
                message: format!("unsafe block `{}` declares no proof obligations", b.name),
                span: b.span.clone(),
                secondary: None,
            })
            .collect()
    }
}

impl Default for UnsafeEscapeChecker {
    fn default() -> Self {
        Self::new()
    }
}
