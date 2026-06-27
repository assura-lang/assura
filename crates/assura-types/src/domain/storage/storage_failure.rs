//! T091: STOR.6 Storage failure model.

use assura_parser::ast::{ClauseKind, Expr};

use crate::TypeError;

#[derive(Debug, Clone)]
pub(crate) struct StorageFailureChecker {
    failure_modes: Vec<FailureMode>,
    handled_modes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FailureMode {
    PartialWrite,
    TornPage,
    BitRot,
    DiskFull,
    IoTimeout,
}

impl FailureMode {
    pub fn name(&self) -> &str {
        match self {
            Self::PartialWrite => "partial_write",
            Self::TornPage => "torn_page",
            Self::BitRot => "bit_rot",
            Self::DiskFull => "disk_full",
            Self::IoTimeout => "io_timeout",
        }
    }
}

impl StorageFailureChecker {
    pub fn new() -> Self {
        Self {
            failure_modes: Vec::new(),
            handled_modes: Vec::new(),
        }
    }

    pub fn declare_failure_mode(&mut self, mode: FailureMode) {
        self.failure_modes.push(mode);
    }

    pub fn mark_handled(&mut self, mode_name: &str) {
        if !self.handled_modes.contains(&mode_name.to_string()) {
            self.handled_modes.push(mode_name.to_string());
        }
    }

    pub fn check_unhandled(&self) -> Vec<TypeError> {
        self.failure_modes
            .iter()
            .filter(|m| !self.handled_modes.contains(&m.name().to_string()))
            .map(|m| TypeError {
                code: "A38001".into(),
                message: format!("storage failure mode `{}` has no handler", m.name()),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_spurious_handlers(&self) -> Vec<TypeError> {
        let declared: Vec<_> = self
            .failure_modes
            .iter()
            .map(|m| m.name().to_string())
            .collect();
        self.handled_modes
            .iter()
            .filter(|h| !declared.contains(h))
            .map(|h| TypeError {
                code: "A38002".into(),
                message: format!("handler for undeclared failure mode `{h}`"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_critical_coverage(&self) -> Vec<TypeError> {
        let critical = [FailureMode::PartialWrite, FailureMode::TornPage];
        critical
            .iter()
            .filter(|m| {
                self.failure_modes.contains(m)
                    && !self.handled_modes.contains(&m.name().to_string())
            })
            .map(|m| TypeError {
                code: "A38003".into(),
                message: format!("critical failure mode `{}` must have a handler", m.name()),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }
}

impl StorageFailureChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = Self::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "failure_mode" || k == "storage_failure" {
                        found = true;
                        if let Expr::Ident(name) = &clause.body.node {
                            let mode = match name.as_str() {
                                "partial_write" => FailureMode::PartialWrite,
                                "torn_page" => FailureMode::TornPage,
                                "bit_rot" => FailureMode::BitRot,
                                "disk_full" => FailureMode::DiskFull,
                                "io_timeout" => FailureMode::IoTimeout,
                                _ => continue,
                            };
                            checker.declare_failure_mode(mode);
                        }
                    }
                    if (k == "handles" || k == "handles_failure")
                        && let Expr::Ident(name) = &clause.body.node
                    {
                        checker.mark_handled(name);
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = checker.check_unhandled();
        errors.extend(checker.check_critical_coverage());
        errors.extend(checker.check_spurious_handlers());
        errors
    }
}

impl Default for StorageFailureChecker {
    fn default() -> Self {
        Self::new()
    }
}
