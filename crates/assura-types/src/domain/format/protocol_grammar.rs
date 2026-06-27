// ===========================================================================
// T075: FMT.6 Protocol grammar
// ===========================================================================

use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{ClauseKind, Decl, Expr};

use crate::TypeError;
use crate::checkers::*;
use crate::domain::OpaqueFunctionChecker;

/// Validates protocol state machine and RFC conformance.
///
/// Error codes:
/// - A30001: invalid state transition
/// - A30002: message sent in wrong protocol state
/// - A30003: required message field missing
#[derive(Debug, Clone)]
pub(crate) struct ProtocolGrammarChecker {
    states: Vec<String>,
    current_state: String,
    transitions: Vec<ProtocolTransition>,
    required_fields: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProtocolTransition {
    pub from: String,
    pub to: String,
    pub message: String,
}

impl ProtocolGrammarChecker {
    pub fn new(initial_state: String) -> Self {
        Self {
            states: vec![initial_state.clone()],
            current_state: initial_state,
            transitions: Vec::new(),
            required_fields: HashMap::new(),
        }
    }

    pub fn add_state(&mut self, state: String) {
        if !self.states.contains(&state) {
            self.states.push(state);
        }
    }

    pub fn add_transition(&mut self, from: String, to: String, message: String) {
        self.transitions
            .push(ProtocolTransition { from, to, message });
    }

    pub fn add_required_fields(&mut self, message: String, fields: Vec<String>) {
        self.required_fields.insert(message, fields);
    }

    pub fn check_send(&self, message: &str, span: &Range<usize>) -> Option<TypeError> {
        let valid = self
            .transitions
            .iter()
            .any(|t| t.from == self.current_state && t.message == message);
        if !valid {
            return Some(TypeError {
                code: "A30002".into(),
                message: format!("cannot send `{message}` in state `{}`", self.current_state),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
        None
    }

    pub fn transition(&mut self, message: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(t) = self
            .transitions
            .iter()
            .find(|t| t.from == self.current_state && t.message == message)
        {
            self.current_state = t.to.clone();
            None
        } else {
            Some(TypeError {
                code: "A30001".into(),
                message: format!(
                    "invalid transition: no `{message}` transition from state `{}`",
                    self.current_state
                ),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            })
        }
    }

    pub fn check_required_fields(
        &self,
        message: &str,
        provided: &[&str],
        span: &Range<usize>,
    ) -> Vec<TypeError> {
        let mut errors = Vec::new();
        if let Some(required) = self.required_fields.get(message) {
            for field in required {
                if !provided.contains(&field.as_str()) {
                    errors.push(TypeError {
                        code: "A30003".into(),
                        message: format!("required field `{field}` missing in message `{message}`"),
                        span: span.clone(),
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }
}

impl ProtocolGrammarChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker: Option<ProtocolGrammarChecker> = None;
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "protocol" || k == "state_machine" || k == "rfc" {
                        found = true;
                        let initial = extract_ident(&clause.body).unwrap_or("init").to_string();
                        if checker.is_none() {
                            checker = Some(ProtocolGrammarChecker::new(initial));
                        }
                    }
                    if (k == "state" || k == "protocol_state")
                        && let Some(name) = extract_ident(&clause.body)
                        && let Some(ref mut ch) = checker
                    {
                        ch.add_state(name.to_string());
                    }
                    if k == "transition"
                        && let Some((from, args)) = extract_call(&clause.body)
                        && args.len() >= 2
                        && let Some(ref mut ch) = checker
                    {
                        let msg = extract_ident(&args[0]).unwrap_or("unknown").to_string();
                        let to = extract_ident(&args[1]).unwrap_or("unknown").to_string();
                        ch.add_transition(from.to_string(), to, msg);
                    }
                    if (k == "required_fields" || k == "required")
                        && let Some((msg, args)) = extract_call(&clause.body)
                        && let Some(ref mut ch) = checker
                    {
                        let field_names: Vec<String> = args
                            .iter()
                            .filter_map(|a| extract_ident(a).map(String::from))
                            .collect();
                        ch.add_required_fields(msg.to_string(), field_names);
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let checker = match checker {
            Some(c) => c,
            None => return Vec::new(),
        };
        let mut checker = checker;
        let mut errors = Vec::new();
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "send" || k == "message")
                    && let Some(msg) = extract_ident(&clause.body)
                {
                    if let Some(err) = checker.check_send(msg, &decl.span) {
                        errors.push(err);
                    }
                    if let Some(err) = checker.transition(msg, &decl.span) {
                        errors.push(err);
                    }
                    let field_errs = checker.check_required_fields(msg, &[], &decl.span);
                    errors.extend(field_errs);
                }
            }
        }
        errors
    }
}

impl OpaqueFunctionChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = OpaqueFunctionChecker::new();
        let mut found = false;
        for decl in &source.decls {
            if let Decl::FnDef(f) = &decl.node {
                for clause in &f.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "opaque"
                    {
                        found = true;
                        let has_contract = f
                            .clauses
                            .iter()
                            .any(|c| matches!(c.kind, ClauseKind::Requires | ClauseKind::Ensures));
                        checker.declare_opaque(f.name.clone(), has_contract, decl.span.clone());
                    }
                }
            } else if let Decl::Contract(c) = &decl.node {
                for clause in &c.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "opaque"
                    {
                        found = true;
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = Vec::new();
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "proof" || k == "proof_context" {
                        checker.enter_proof();
                    }
                    if k == "end_proof" {
                        checker.exit_proof();
                    }
                    if k == "reveal"
                        && let Expr::Ident(fn_name) = &clause.body.node
                        && let Some(err) = checker.reveal(fn_name, &decl.span)
                    {
                        errors.push(err);
                    }
                }
                if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        if let Some(err) = checker.check_call(name, &decl.span) {
                            errors.push(err);
                        }
                        if checker.is_opaque(name)
                            && let Some(mut err) = checker.check_body_access(name, &decl.span)
                        {
                            err.secondary = checker.opaque_span(name).map(|s| {
                                (s.clone(), format!("opaque function `{name}` declared here"))
                            });
                            errors.push(err);
                        }
                    }
                }
            }
        }
        errors
    }
}
