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

impl Default for EffectChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
