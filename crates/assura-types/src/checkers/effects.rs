use super::*;

// Effect checking (T036)
// ---------------------------------------------------------------------------

/// A set of effects declared on (or inferred for) a function.
///
/// Effects are stored as lowercase strings matching the effect labels from
/// Section 3.1 of the spec (e.g., `"io"`, `"console.read"`, `"pure"`).
/// The special value `"pure"` represents an empty effect set.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EffectSet {
    effects: std::collections::HashSet<String>,
}

impl EffectSet {
    /// Create a new empty effect set (equivalent to `pure`).
    pub fn pure() -> Self {
        Self {
            effects: std::collections::HashSet::new(),
        }
    }

    /// Create an effect set from an iterator of effect names.
    ///
    /// The name `"pure"` is treated as an empty set; it is not stored as
    /// an actual effect label.
    pub fn from_effect_names(effects: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut set = std::collections::HashSet::new();
        for e in effects {
            let name = e.into();
            if name != "pure" {
                set.insert(name);
            }
        }
        Self { effects: set }
    }

    /// Returns `true` if this is a pure (empty) effect set.
    pub fn is_pure(&self) -> bool {
        self.is_empty()
    }

    /// Insert an effect into the set.
    pub fn insert(&mut self, effect: String) {
        if effect != "pure" {
            self.effects.insert(effect);
        }
    }

    /// Returns `true` if the set contains the given effect.
    pub fn contains(&self, effect: &str) -> bool {
        self.effects.contains(effect)
    }

    /// Iterate over the effect names in this set.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.effects.iter().map(|s| s.as_str())
    }

    /// Number of effects in the set.
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Returns `true` if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}

impl std::fmt::Display for EffectSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            return write!(f, "pure");
        }
        let mut sorted: Vec<&str> = self.effects.iter().map(|s| s.as_str()).collect();
        sorted.sort();
        write!(f, "{{{}}}", sorted.join(", "))
    }
}

pub(crate) type EffectError = CheckerError;

/// Effect checker that validates effect declarations and containment.
///
/// Implements the effect checking rules from Section 3.5 of the spec:
/// a function's body may only use effects declared in its signature,
/// and all effect names must be recognized (built-in or user-defined).
///
/// The effect hierarchy from Section 3.6 is encoded: `io` is shorthand
/// for all IO sub-effects, `database` for all database sub-effects,
/// and `logging` for all log sub-effects.
pub(crate) struct EffectChecker {
    /// All known effect names (both group names and leaf effects).
    known_effects: std::collections::HashSet<&'static str>,
    /// Maps a group effect to its sub-effects.
    hierarchy: HashMap<&'static str, Vec<&'static str>>,
}

impl EffectChecker {
    /// Create a new effect checker with the built-in effect vocabulary
    /// from Section 3.1 and hierarchy from Section 3.6 of the spec.
    pub fn new() -> Self {
        let known: std::collections::HashSet<&'static str> = [
            // Group effects
            "io",
            "database",
            "logging",
            // Leaf IO effects
            "console.read",
            "console.write",
            "filesystem.read",
            "filesystem.write",
            "network.connect",
            "network.send",
            "network.receive",
            "time.read",
            "random",
            // Leaf database effects
            "database.read",
            "database.write",
            // Leaf logging effects
            "log.debug",
            "log.info",
            "log.warn",
            "log.error",
            // Other built-in effects
            "diverge",
            // Memory effect (from AGENTS.md task description)
            "mem",
            "net",
            "fs",
            "rng",
            "time",
            "alloc",
        ]
        .into_iter()
        .collect();

        let mut hierarchy = HashMap::new();
        hierarchy.insert(
            "io",
            vec![
                "console.read",
                "console.write",
                "filesystem.read",
                "filesystem.write",
                "network.connect",
                "network.send",
                "network.receive",
                "time.read",
                "random",
                // Short aliases that map to IO sub-categories
                "net",
                "fs",
                "rng",
                "time",
            ],
        );
        hierarchy.insert("database", vec!["database.read", "database.write"]);
        hierarchy.insert(
            "logging",
            vec!["log.debug", "log.info", "log.warn", "log.error"],
        );
        // Short alias groups
        hierarchy.insert(
            "net",
            vec!["network.connect", "network.send", "network.receive"],
        );
        hierarchy.insert("fs", vec!["filesystem.read", "filesystem.write"]);

        Self {
            known_effects: known,
            hierarchy,
        }
    }

    /// Expand a declared effect set by adding all sub-effects implied by
    /// the hierarchy. For example, declaring `io` expands to include
    /// `console.read`, `console.write`, etc.
    pub fn expand(&self, declared: &EffectSet) -> EffectSet {
        let mut expanded = declared.clone();
        // Iterate over the original set (not the expanding one) to avoid
        // borrow issues.
        let originals: Vec<String> = declared.effects.iter().cloned().collect();
        for effect in &originals {
            if let Some(children) = self.hierarchy.get(effect.as_str()) {
                for &child in children {
                    expanded.insert(child.to_string());
                }
            }
        }
        expanded
    }

    /// Check that all effects in `actual` are contained in `declared`.
    ///
    /// The `declared` set is expanded via the hierarchy before comparison.
    /// Returns a list of `EffectError`s for violations:
    ///
    /// - **A07001**: An effect in `actual` is not present in the expanded
    ///   `declared` set (undeclared effect).
    /// - **A07002**: The function is declared `pure` (empty declared set)
    ///   but the body performs effects (side effect in pure context).
    pub fn check_containment(
        &self,
        declared: &EffectSet,
        actual: &EffectSet,
        span: &Range<usize>,
    ) -> Vec<EffectError> {
        let mut errors = Vec::new();

        // Expand the declared set to include sub-effects
        let expanded = self.expand(declared);

        for effect in actual.iter() {
            // Check if the actual effect (or a parent of it) is in the
            // expanded declared set.
            if !self.is_allowed(effect, &expanded) {
                if declared.is_pure() {
                    // A07002: pure function performs effect
                    errors.push(EffectError {
                        code: "A07002".into(),
                        message: format!(
                            "pure function performs effect `{effect}`: \
                             side effects are not allowed in a pure context"
                        ),
                        span: span.clone(),
                    });
                } else {
                    // A07001: undeclared effect
                    errors.push(EffectError {
                        code: "A07001".into(),
                        message: format!(
                            "undeclared effect `{effect}`: \
                             effect not in function's declared effect set \
                             {declared} ({} declared)",
                            declared.len()
                        ),
                        span: span.clone(),
                    });
                }
            }
        }

        // Sort errors by code then message for deterministic output.
        errors.sort_by(|a, b| a.code.cmp(&b.code).then(a.message.cmp(&b.message)));
        errors
    }

    /// Check that all effect names in a set are recognized.
    ///
    /// Returns A07003 errors for unknown effect names.
    pub fn check_known(&self, effects: &EffectSet, span: &Range<usize>) -> Vec<EffectError> {
        let mut errors = Vec::new();

        for effect in effects.iter() {
            // Skip identifiers that are clearly not effect names:
            // - Capitalized names (type names like `InflateDecoder`)
            // - Known block-kind keywords that leak from parser spans
            // This prevents false positives from parser artifacts where
            // block kind names leak into effect clause token streams.
            if effect.chars().next().is_some_and(|c| c.is_uppercase()) {
                continue;
            }
            if is_block_kind_keyword(effect) {
                continue;
            }
            if !self.is_known(effect) && !self.is_sub_effect_of_known(effect) {
                errors.push(EffectError {
                    code: "A07003".into(),
                    message: format!("unknown effect name `{effect}`"),
                    span: span.clone(),
                });
            }
        }

        errors.sort_by(|a, b| a.message.cmp(&b.message));
        errors
    }

    /// Returns `true` if the effect is a dot-separated sub-effect of a
    /// known group. For example, `io.read` is accepted because `io` is
    /// a known group effect.
    fn is_sub_effect_of_known(&self, effect: &str) -> bool {
        if let Some(dot_pos) = effect.find('.') {
            let parent = &effect[..dot_pos];
            self.known_effects.contains(parent) || self.hierarchy.contains_key(parent)
        } else {
            false
        }
    }
}

/// Returns `true` if the name is a known Assura block-kind keyword
/// (e.g., `incremental`, `feature`, `liveness`) that should not be
/// treated as an effect name even when it appears in an effect clause
/// due to parser span overlap.
fn is_block_kind_keyword(name: &str) -> bool {
    matches!(
        name,
        "incremental"
            | "feature"
            | "liveness"
            | "axiomatic"
            | "axiom"
            | "lemma"
            | "ghost"
            | "opaque"
            | "test"
            | "property"
            | "complexity"
            | "benchmark"
            | "migration"
    )
}

impl EffectChecker {
    /// Returns `true` if `effect` is allowed by the expanded declared set.
    ///
    /// An effect is allowed if:
    /// 1. It is directly in the expanded set, OR
    /// 2. Any of its ancestor groups are in the expanded set.
    fn is_allowed(&self, effect: &str, expanded: &EffectSet) -> bool {
        // Direct containment
        if expanded.contains(effect) {
            return true;
        }

        // Check if any group in the expanded set subsumes this effect
        for group_effect in expanded.iter() {
            if let Some(children) = self.hierarchy.get(group_effect)
                && children.contains(&effect)
            {
                return true;
            }
        }

        false
    }

    /// Returns `true` if the given effect name is a known built-in effect.
    pub fn is_known(&self, effect: &str) -> bool {
        self.known_effects.contains(effect)
    }
}

impl EffectChecker {
    /// Full AST-walking entry point for effect checking.
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let checker = EffectChecker::new();
        let mut errors = Vec::new();
        let effect_map = Self::build_effect_map_from(source, &checker);
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_extern(&decl.node) else {
                continue;
            };
            let (declared, actual) = Self::extract_effects_from_clauses(clauses);
            if let Some(ref declared_set) = declared {
                for ee in checker.check_known(declared_set, &decl.span) {
                    errors.push(TypeError {
                        code: ee.code,
                        message: ee.message,
                        span: ee.span,
                        secondary: None,
                    });
                }
                if matches!(&decl.node, Decl::FnDef(_)) {
                    if let Some(actual_set) = actual {
                        for ee in checker.check_containment(declared_set, &actual_set, &decl.span) {
                            errors.push(TypeError {
                                code: ee.code,
                                message: ee.message,
                                span: ee.span,
                                secondary: None,
                            });
                        }
                    }
                    let callee_effects = Self::infer_callee_effects(clauses, &effect_map);
                    for ee in checker.check_containment(declared_set, &callee_effects, &decl.span) {
                        errors.push(TypeError {
                            code: ee.code,
                            message: ee.message,
                            span: ee.span,
                            secondary: None,
                        });
                    }
                }
            }
        }
        errors
    }

    /// Build a map from function/contract/extern names to their declared effect sets.
    pub fn build_effect_map_from(
        source: &assura_parser::ast::SourceFile,
        checker: &EffectChecker,
    ) -> HashMap<String, EffectSet> {
        let mut map = HashMap::new();
        for decl in &source.decls {
            if let Some(clauses) = crate::checks::clauses_contract_fn_extern(&decl.node) {
                let (declared, _) = Self::extract_effects_from_clauses(clauses);
                if let Some(declared_set) = declared
                    && let Some(name) = decl.node.name()
                {
                    map.insert(name.to_string(), checker.expand(&declared_set));
                }
            } else if let Decl::Service(s) = &decl.node {
                for item in &s.items {
                    if let ServiceItem::Operation { name, clauses, .. } = item {
                        let (declared, _) = Self::extract_effects_from_clauses(clauses);
                        if let Some(declared_set) = declared {
                            map.insert(name.clone(), checker.expand(&declared_set));
                        }
                    }
                }
            }
        }
        map
    }

    /// Extract declared and actual effect sets from a list of clauses.
    pub fn extract_effects_from_clauses(
        clauses: &[assura_parser::ast::Clause],
    ) -> (Option<EffectSet>, Option<EffectSet>) {
        let mut declared: Option<EffectSet> = None;
        let mut actual: Option<EffectSet> = None;
        for clause in clauses {
            if clause.kind == ClauseKind::Effects {
                let effects = Self::extract_effect_names_from_expr(&clause.body);
                declared = Some(EffectSet::from_effect_names(effects));
            }
        }
        let mut inferred = EffectSet::pure();
        for clause in clauses {
            if matches!(
                clause.kind,
                ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Modifies
            ) {
                Self::infer_effects_from_expr(&clause.body, &mut inferred);
            }
        }
        if !inferred.is_pure() {
            actual = Some(inferred);
        }
        (declared, actual)
    }

    fn infer_callee_effects(
        clauses: &[assura_parser::ast::Clause],
        effect_map: &HashMap<String, EffectSet>,
    ) -> EffectSet {
        let mut result = EffectSet::pure();
        for clause in clauses {
            if matches!(
                clause.kind,
                ClauseKind::Requires
                    | ClauseKind::Ensures
                    | ClauseKind::Modifies
                    | ClauseKind::Invariant
                    | ClauseKind::Rule
            ) {
                Self::collect_call_effects(&clause.body, effect_map, &mut result);
            }
        }
        result
    }

    fn collect_call_effects(
        expr: &SpExpr,
        effect_map: &HashMap<String, EffectSet>,
        effects: &mut EffectSet,
    ) {
        match &expr.node {
            Expr::Call { func, args } => {
                if let Some(name) = Self::extract_call_name(func)
                    && let Some(callee_effects) = effect_map.get(&name)
                {
                    for eff in callee_effects.iter() {
                        effects.insert(eff.to_string());
                    }
                }
                for arg in args {
                    Self::collect_call_effects(arg, effect_map, effects);
                }
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                if let Some(callee_effects) = effect_map.get(method.as_str()) {
                    for eff in callee_effects.iter() {
                        effects.insert(eff.to_string());
                    }
                }
                Self::collect_call_effects(receiver, effect_map, effects);
                for arg in args {
                    Self::collect_call_effects(arg, effect_map, effects);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                Self::collect_call_effects(lhs, effect_map, effects);
                Self::collect_call_effects(rhs, effect_map, effects);
            }
            Expr::UnaryOp { expr: inner, .. } => {
                Self::collect_call_effects(inner, effect_map, effects);
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                Self::collect_call_effects(cond, effect_map, effects);
                Self::collect_call_effects(then_branch, effect_map, effects);
                if let Some(el) = else_branch {
                    Self::collect_call_effects(el, effect_map, effects);
                }
            }
            Expr::Block(items) | Expr::List(items) | Expr::Tuple(items) => {
                for item in items {
                    Self::collect_call_effects(item, effect_map, effects);
                }
            }
            Expr::Forall { body, domain, .. } | Expr::Exists { body, domain, .. } => {
                Self::collect_call_effects(body, effect_map, effects);
                Self::collect_call_effects(domain, effect_map, effects);
            }
            Expr::Old(inner)
            | Expr::Ghost(inner)
            | Expr::Field(inner, _)
            | Expr::Cast { expr: inner, .. } => {
                Self::collect_call_effects(inner, effect_map, effects);
            }
            Expr::Index { expr: base, index } => {
                Self::collect_call_effects(base, effect_map, effects);
                Self::collect_call_effects(index, effect_map, effects);
            }
            Expr::Apply { args, .. } => {
                for arg in args {
                    Self::collect_call_effects(arg, effect_map, effects);
                }
            }
            Expr::Let { value, body, .. } => {
                Self::collect_call_effects(value, effect_map, effects);
                Self::collect_call_effects(body, effect_map, effects);
            }
            Expr::Match { scrutinee, arms } => {
                Self::collect_call_effects(scrutinee, effect_map, effects);
                for arm in arms {
                    Self::collect_call_effects(&arm.body, effect_map, effects);
                }
            }
            Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
        }
    }

    fn extract_call_name(func: &SpExpr) -> Option<String> {
        match &func.node {
            Expr::Ident(name) => Some(name.clone()),
            Expr::Field(_, name) => Some(name.clone()),
            _ => None,
        }
    }

    fn extract_effect_names_from_expr(expr: &SpExpr) -> Vec<String> {
        match &expr.node {
            Expr::Ident(name) => vec![name.clone()],
            Expr::Raw(tokens) => {
                let filtered: Vec<&str> = tokens
                    .iter()
                    .map(|s| s.as_str())
                    .filter(|t| !matches!(*t, "," | "{" | "}" | "<" | ">" | "|"))
                    .collect();
                let mut names = Vec::new();
                let mut current = String::new();
                for tok in filtered {
                    if tok == "." {
                        current.push('.');
                    } else if current.ends_with('.') {
                        current.push_str(tok);
                    } else {
                        if !current.is_empty() {
                            names.push(current);
                        }
                        current = tok.to_string();
                    }
                }
                if !current.is_empty() {
                    names.push(current);
                }
                names
            }
            Expr::Block(items) => items
                .iter()
                .flat_map(Self::extract_effect_names_from_expr)
                .collect(),
            Expr::Field(base, field) => {
                let mut base_names = Self::extract_effect_names_from_expr(base);
                if let Some(last) = base_names.last_mut() {
                    last.push('.');
                    last.push_str(field);
                } else {
                    base_names.push(field.clone());
                }
                base_names
            }
            _ => Vec::new(),
        }
    }

    fn infer_effects_from_expr(expr: &SpExpr, effects: &mut EffectSet) {
        match &expr.node {
            Expr::Ident(name) => {
                let io_prefixes = [
                    "console",
                    "file",
                    "stdin",
                    "stdout",
                    "stderr",
                    "network",
                    "socket",
                    "http",
                    "tcp",
                    "udp",
                    "process",
                    "env",
                    "time",
                    "random",
                    "rand",
                    "print",
                    "read_line",
                    "write_file",
                    "read_file",
                    "open",
                    "close",
                    "flush",
                    "seek",
                ];
                for prefix in &io_prefixes {
                    if name.starts_with(prefix) || name == *prefix {
                        effects.insert("io".into());
                        return;
                    }
                }
                if name.starts_with("alloc")
                    || name.starts_with("dealloc")
                    || name.starts_with("malloc")
                    || name.starts_with("free")
                    || name.starts_with("realloc")
                    || name.starts_with("resize")
                {
                    effects.insert("mem".into());
                }
                if name == "panic"
                    || name == "abort"
                    || name == "unreachable"
                    || name == "exit"
                    || name == "todo"
                {
                    effects.insert("panic".into());
                }
            }
            Expr::Field(base, field) => {
                let io_methods = [
                    "read",
                    "write",
                    "flush",
                    "close",
                    "open",
                    "seek",
                    "send",
                    "recv",
                    "connect",
                    "listen",
                    "accept",
                    "print",
                    "println",
                    "read_line",
                ];
                if io_methods.contains(&field.as_str()) {
                    effects.insert("io".into());
                }
                Self::infer_effects_from_expr(base, effects);
            }
            Expr::Call { func, args } => {
                Self::infer_effects_from_expr(func, effects);
                for a in args {
                    Self::infer_effects_from_expr(a, effects);
                }
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                let io_methods = [
                    "read",
                    "write",
                    "flush",
                    "close",
                    "open",
                    "seek",
                    "send",
                    "recv",
                    "connect",
                    "listen",
                    "accept",
                    "print",
                    "println",
                    "read_line",
                ];
                if io_methods.contains(&method.as_str()) {
                    effects.insert("io".into());
                }
                Self::infer_effects_from_expr(receiver, effects);
                for a in args {
                    Self::infer_effects_from_expr(a, effects);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                Self::infer_effects_from_expr(lhs, effects);
                Self::infer_effects_from_expr(rhs, effects);
            }
            Expr::UnaryOp { expr, .. } | Expr::Old(expr) => {
                Self::infer_effects_from_expr(expr, effects);
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                Self::infer_effects_from_expr(cond, effects);
                Self::infer_effects_from_expr(then_branch, effects);
                if let Some(e) = else_branch {
                    Self::infer_effects_from_expr(e, effects);
                }
            }
            Expr::Block(items) | Expr::List(items) => {
                for item in items {
                    Self::infer_effects_from_expr(item, effects);
                }
            }
            _ => {}
        }
    }
}

impl Default for EffectChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Range<usize> {
        0..10
    }

    // -- EffectSet --

    #[test]
    fn pure_effect_set_is_empty() {
        let es = EffectSet::pure();
        assert!(es.is_pure());
        assert!(es.is_empty());
        assert_eq!(es.len(), 0);
        assert_eq!(es.to_string(), "pure");
    }

    #[test]
    fn from_effect_names_filters_pure() {
        let es = EffectSet::from_effect_names(vec!["io", "pure", "database"]);
        assert_eq!(es.len(), 2);
        assert!(es.contains("io"));
        assert!(es.contains("database"));
        assert!(!es.contains("pure"));
    }

    #[test]
    fn insert_and_contains() {
        let mut es = EffectSet::pure();
        es.insert("io".into());
        assert!(es.contains("io"));
        assert!(!es.is_pure());
    }

    // -- EffectChecker --

    #[test]
    fn expand_io_includes_sub_effects() {
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(vec!["io"]);
        let expanded = checker.expand(&declared);
        assert!(expanded.contains("console.read"));
        assert!(expanded.contains("network.send"));
        assert!(expanded.contains("filesystem.write"));
    }

    #[test]
    fn containment_declared_io_allows_console_read() {
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(vec!["io"]);
        let actual = EffectSet::from_effect_names(vec!["console.read"]);
        let errs = checker.check_containment(&declared, &actual, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn containment_undeclared_effect_a07001() {
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(vec!["database"]);
        let actual = EffectSet::from_effect_names(vec!["io"]);
        let errs = checker.check_containment(&declared, &actual, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A07001");
    }

    #[test]
    fn containment_pure_with_effects_a07002() {
        let checker = EffectChecker::new();
        let declared = EffectSet::pure();
        let actual = EffectSet::from_effect_names(vec!["io"]);
        let errs = checker.check_containment(&declared, &actual, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A07002");
    }

    #[test]
    fn containment_pure_with_pure_ok() {
        let checker = EffectChecker::new();
        let declared = EffectSet::pure();
        let actual = EffectSet::pure();
        let errs = checker.check_containment(&declared, &actual, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn check_known_accepts_builtin() {
        let checker = EffectChecker::new();
        let es = EffectSet::from_effect_names(vec!["io", "database", "mem"]);
        let errs = checker.check_known(&es, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn check_known_rejects_unknown_a07003() {
        let checker = EffectChecker::new();
        let es = EffectSet::from_effect_names(vec!["teleport"]);
        let errs = checker.check_known(&es, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A07003");
    }

    #[test]
    fn check_known_accepts_dotted_sub_effect() {
        let checker = EffectChecker::new();
        // "io.custom" should be accepted because "io" is a known group
        let es = EffectSet::from_effect_names(vec!["io.custom"]);
        let errs = checker.check_known(&es, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn is_block_kind_keyword_filtered() {
        let checker = EffectChecker::new();
        let es = EffectSet::from_effect_names(vec!["feature", "axiom"]);
        let errs = checker.check_known(&es, &span());
        // block-kind keywords should be silently skipped, not flagged
        assert!(errs.is_empty());
    }
}

// ---------------------------------------------------------------------------
