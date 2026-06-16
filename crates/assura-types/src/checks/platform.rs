//! Platform-related checks.
//!
//! Platform abstraction, feature flags, resource limits.

use assura_parser::ast::{ClauseKind, Decl, Expr};

use crate::TypeError;
use crate::checkers::*;
use crate::domain::*;
use crate::types::*;

pub(crate) fn run_platform_abstraction_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = PlatformAbstractionChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "platform" || k == "target_platform" {
                    found = true;
                    if let Expr::Ident(name) = &clause.body {
                        checker.add_platform(name.clone());
                    }
                }
                if k == "abstraction" || k == "platform_abstraction" {
                    match &clause.body {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = func.as_ref() {
                                let platforms: Vec<String> = args
                                    .iter()
                                    .filter_map(|a| extract_ident(a).map(String::from))
                                    .collect();
                                checker.declare_abstraction(name.clone(), platforms);
                            }
                        }
                        Expr::Ident(name) => {
                            // Collect platforms declared so far as supported
                            let platforms = checker.known_platforms().to_vec();
                            checker.declare_abstraction(name.clone(), platforms);
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name" || *k == "abstraction")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed")
                                .to_string();
                            let platforms: Vec<String> = kvs
                                .iter()
                                .filter(|(k, _)| *k == "platform" || *k == "supports")
                                .filter_map(|(_, v)| extract_ident(v).map(String::from))
                                .collect();
                            checker.declare_abstraction(name, platforms);
                        }
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = checker.check_coverage();
    errors.extend(checker.check_unknown_platforms());
    // Check for direct platform use in clause bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_direct_platform_use(name) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

/// Scan for feature flag annotations.
pub(crate) fn run_feature_flag_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = FeatureFlagChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "feature_flag"
            {
                found = true;
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
                            let enabled = args
                                .first()
                                .and_then(extract_ident)
                                .is_some_and(|v| v == "true" || v == "enabled" || v == "on");
                            let deps: Vec<String> = args
                                .iter()
                                .skip(1)
                                .filter_map(|a| extract_ident(a).map(String::from))
                                .collect();
                            checker.declare(name.clone(), enabled, deps);
                        }
                    }
                    Expr::Ident(name) => {
                        checker.declare(name.clone(), false, Vec::new());
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "flag")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let enabled = kvs
                            .iter()
                            .find(|(k, _)| *k == "default" || *k == "enabled")
                            .and_then(|(_, v)| extract_ident(v))
                            .is_some_and(|v| v == "true" || v == "enabled" || v == "on");
                        let deps: Vec<String> = kvs
                            .iter()
                            .filter(|(k, _)| *k == "depends_on" || *k == "requires")
                            .filter_map(|(_, v)| extract_ident(v).map(String::from))
                            .collect();
                        checker.declare(name, enabled, deps);
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Mark flags as used and check for undeclared references in clause bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    checker.mark_used(name);
                }
            }
            // Check for undeclared flag references
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "when_flag" || k == "if_feature")
                && let Expr::Ident(flag_name) = &clause.body
                && let Some(err) = checker.check_undeclared(flag_name)
            {
                return vec![err];
            }
        }
    }
    let mut errors = checker.check_unused();
    errors.extend(checker.check_conflicts());
    errors
}

/// Scan for resource limit annotations.
pub(crate) fn run_resource_limit_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = ResourceLimitChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "resource_limit" || k == "limit" || k == "quota")
            {
                found = true;
                // Extract limit: limit(name, max, unit)
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
                            let max_val =
                                args.first()
                                    .and_then(extract_int_literal)
                                    .unwrap_or(i64::MAX) as u64;
                            let unit = args
                                .get(1)
                                .and_then(extract_ident)
                                .unwrap_or("units")
                                .to_string();
                            checker.declare_limit(name.clone(), max_val, unit);
                        }
                    }
                    Expr::Ident(name) => {
                        // Bare identifier without explicit max: flag as unbounded via check_unbounded
                        checker.declare_limit(name.clone(), u64::MAX, "units".into());
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "resource")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let max_val = kvs
                            .iter()
                            .find(|(k, _)| *k == "max" || *k == "limit")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(i64::MAX) as u64;
                        let unit = kvs
                            .iter()
                            .find(|(k, _)| *k == "unit" || *k == "units")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("units")
                            .to_string();
                        checker.declare_limit(name, max_val, unit);
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = Vec::new();
    // Track resource usage and release from clause bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if (k == "use_resource" || k == "consume")
                    && let Some((name, args)) = extract_call(&clause.body)
                {
                    let amount = args
                        .first()
                        .and_then(extract_int_literal)
                        .unwrap_or(DEFAULT_PARAM_ONE) as u64;
                    checker.record_usage(name, amount);
                }
                if (k == "release_resource" || k == "free_resource")
                    && let Some((name, args)) = extract_call(&clause.body)
                {
                    let amount = args
                        .first()
                        .and_then(extract_int_literal)
                        .unwrap_or(DEFAULT_PARAM_ONE) as u64;
                    checker.release_usage(name, amount);
                }
            }
            // Check for unbounded resource references in clause bodies
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_unbounded(name) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors.extend(checker.check_limits());
    errors.extend(checker.check_near_limit());
    errors
}
