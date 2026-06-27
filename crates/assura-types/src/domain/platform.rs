//! Platform-related domain checkers.
//!
//! PlatformAbstractionChecker, FeatureFlagChecker, ResourceLimitChecker.

use assura_parser::ast::{ClauseKind, Expr};

use crate::TypeError;
use crate::checkers::*;
use crate::types::*;

// ===========================================================================
// T097: PLAT.1 Platform abstraction
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct PlatformAbstractionChecker {
    platforms: Vec<String>,
    abstractions: std::collections::HashMap<String, Vec<String>>,
}

impl PlatformAbstractionChecker {
    pub fn new() -> Self {
        Self {
            platforms: Vec::new(),
            abstractions: std::collections::HashMap::new(),
        }
    }

    pub fn known_platforms(&self) -> &[String] {
        &self.platforms
    }

    pub fn add_platform(&mut self, name: String) {
        if !self.platforms.contains(&name) {
            self.platforms.push(name);
        }
    }

    pub fn declare_abstraction(&mut self, name: String, supported: Vec<String>) {
        self.abstractions.insert(name, supported);
    }

    pub fn check_coverage(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, supported) in &self.abstractions {
            for platform in &self.platforms {
                if !supported.contains(platform) {
                    errors.push(TypeError {
                        code: "A44001".into(),
                        message: format!(
                            "abstraction `{name}` missing impl for platform `{platform}`"
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

    pub fn check_direct_platform_use(&self, used_platform: &str) -> Option<TypeError> {
        if self.platforms.contains(&used_platform.to_string()) {
            Some(TypeError {
                code: "A44002".into(),
                message: format!("direct platform reference `{used_platform}` without abstraction"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
        } else {
            None
        }
    }

    pub fn check_unknown_platforms(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, supported) in &self.abstractions {
            for p in supported {
                if !self.platforms.contains(p) {
                    errors.push(TypeError {
                        code: "A44003".into(),
                        message: format!("abstraction `{name}` references unknown platform `{p}`"),
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

impl PlatformAbstractionChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = Self::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "platform" || k == "target_platform" {
                        found = true;
                        if let Expr::Ident(name) = &clause.body.node {
                            checker.add_platform(name.clone());
                        }
                    }
                    if k == "abstraction" || k == "platform_abstraction" {
                        match &clause.body.node {
                            Expr::Call { func, args } => {
                                if let Expr::Ident(name) = &func.as_ref().node {
                                    let platforms: Vec<String> = args
                                        .iter()
                                        .filter_map(|a| extract_ident(a).map(String::from))
                                        .collect();
                                    checker.declare_abstraction(name.clone(), platforms);
                                }
                            }
                            Expr::Ident(name) => {
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
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn(&decl.node) else {
                continue;
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
}

impl Default for PlatformAbstractionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T098: PLAT.2 Feature flags
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct FeatureFlagChecker {
    flags: std::collections::HashMap<String, FeatureFlagInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct FeatureFlagInfo {
    pub default_enabled: bool,
    pub used: bool,
    pub conflicts_with: Vec<String>,
}

impl FeatureFlagChecker {
    pub fn new() -> Self {
        Self {
            flags: std::collections::HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: String, default_enabled: bool, conflicts_with: Vec<String>) {
        self.flags.insert(
            name,
            FeatureFlagInfo {
                default_enabled,
                used: false,
                conflicts_with,
            },
        );
    }

    pub fn mark_used(&mut self, name: &str) {
        if let Some(f) = self.flags.get_mut(name) {
            f.used = true;
        }
    }

    pub fn check_unused(&self) -> Vec<TypeError> {
        self.flags
            .iter()
            .filter(|(_, i)| !i.used)
            .map(|(n, _)| TypeError {
                code: "A45001".into(),
                message: format!("feature flag `{n}` is declared but never used"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_conflicts(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, info) in &self.flags {
            if info.default_enabled {
                for conflict in &info.conflicts_with {
                    if let Some(other) = self.flags.get(conflict)
                        && other.default_enabled
                    {
                        errors.push(TypeError {
                            code: "A45002".into(),
                            message: format!(
                                "conflicting flags: `{name}` and `{conflict}` both enabled"
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

    pub fn check_undeclared(&self, flag_name: &str) -> Option<TypeError> {
        if !self.flags.contains_key(flag_name) {
            Some(TypeError {
                code: "A45003".into(),
                message: format!("reference to undeclared feature flag `{flag_name}`"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
        } else {
            None
        }
    }
}

impl FeatureFlagChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = Self::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "feature_flag"
                {
                    found = true;
                    match &clause.body.node {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = &func.as_ref().node {
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
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        checker.mark_used(name);
                    }
                }
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "when_flag" || k == "if_feature")
                    && let Expr::Ident(flag_name) = &clause.body.node
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
}

impl Default for FeatureFlagChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T099: PLAT.3 Resource limits
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct ResourceLimitChecker {
    limits: std::collections::HashMap<String, ResourceLimit>,
    usage: std::collections::HashMap<String, u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResourceLimit {
    pub max_value: u64,
    pub unit: String,
}

impl ResourceLimitChecker {
    pub fn new() -> Self {
        Self {
            limits: std::collections::HashMap::new(),
            usage: std::collections::HashMap::new(),
        }
    }

    pub fn declare_limit(&mut self, name: String, max_value: u64, unit: String) {
        self.limits
            .insert(name.clone(), ResourceLimit { max_value, unit });
        self.usage.insert(name, 0);
    }

    pub fn record_usage(&mut self, name: &str, amount: u64) {
        if let Some(u) = self.usage.get_mut(name) {
            *u += amount;
        }
    }

    pub fn release_usage(&mut self, name: &str, amount: u64) {
        if let Some(u) = self.usage.get_mut(name) {
            *u = u.saturating_sub(amount);
        }
    }

    pub fn check_limits(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, limit) in &self.limits {
            if let Some(&current) = self.usage.get(name)
                && current > limit.max_value
            {
                errors.push(TypeError {
                    code: "A46001".into(),
                    message: format!(
                        "resource `{name}` usage {current} exceeds limit {} {}",
                        limit.max_value, limit.unit
                    ),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_unbounded(&self, name: &str) -> Option<TypeError> {
        if !self.limits.contains_key(name) {
            Some(TypeError {
                code: "A46002".into(),
                message: format!("resource `{name}` used without declared limit"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
        } else {
            None
        }
    }

    pub fn check_near_limit(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, limit) in &self.limits {
            if let Some(&current) = self.usage.get(name)
                && limit.max_value > 0
                && current > limit.max_value / 10 * 9
            {
                errors.push(TypeError {
                    code: "A46003".into(),
                    message: format!(
                        "resource `{name}` at {}% of limit",
                        current / (limit.max_value / 100).max(1)
                    ),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }
}

impl ResourceLimitChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = Self::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "resource_limit" || k == "limit" || k == "quota")
                {
                    found = true;
                    match &clause.body.node {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = &func.as_ref().node {
                                let max_val = args
                                    .first()
                                    .and_then(extract_int_literal)
                                    .unwrap_or(i64::MAX)
                                    as u64;
                                let unit = args
                                    .get(1)
                                    .and_then(extract_ident)
                                    .unwrap_or("units")
                                    .to_string();
                                checker.declare_limit(name.clone(), max_val, unit);
                            }
                        }
                        Expr::Ident(name) => {
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
                            let max_val =
                                kvs.iter()
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
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "use_resource" || k == "consume" {
                        if let Some((name, args)) = extract_call(&clause.body) {
                            let amount = args
                                .first()
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_PARAM_ONE)
                                as u64;
                            checker.record_usage(name, amount);
                        } else if let Expr::Ident(name) = &clause.body.node {
                            checker.record_usage(name, 1);
                        } else {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name" || *k == "resource")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed");
                            let amount = kvs
                                .iter()
                                .find(|(k, _)| *k == "amount" || *k == "count")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ONE)
                                as u64;
                            checker.record_usage(name, amount);
                        }
                    }
                    if k == "release_resource" || k == "free_resource" {
                        if let Some((name, args)) = extract_call(&clause.body) {
                            let amount = args
                                .first()
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_PARAM_ONE)
                                as u64;
                            checker.release_usage(name, amount);
                        } else if let Expr::Ident(name) = &clause.body.node {
                            checker.release_usage(name, 1);
                        } else {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name" || *k == "resource")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed");
                            let amount = kvs
                                .iter()
                                .find(|(k, _)| *k == "amount" || *k == "count")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ONE)
                                as u64;
                            checker.release_usage(name, amount);
                        }
                    }
                }
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
}

impl Default for ResourceLimitChecker {
    fn default() -> Self {
        Self::new()
    }
}
