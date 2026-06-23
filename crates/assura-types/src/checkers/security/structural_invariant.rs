use super::*;

// ---------------------------------------------------------------------------
// T063: TYPE.2 Recursive structural invariants
// ---------------------------------------------------------------------------

/// A structural invariant on a recursive data structure.
#[derive(Debug, Clone)]
pub(crate) struct StructuralInvariant {
    pub name: String,
    /// The type this invariant applies to
    pub type_name: String,
    /// Kind of structural property
    pub kind: InvariantKind,
}

/// Kinds of structural invariants for recursive types.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InvariantKind {
    /// Tree balance: left depth and right depth differ by at most k
    TreeBalance { max_diff: u32 },
    /// List sortedness: elements in non-decreasing order
    Sorted { descending: bool },
    /// Graph acyclicity: no cycles in the structure
    Acyclic,
    /// Binary search tree: left < node < right
    BstOrdering,
    /// Heap property: parent <= children (or >=)
    HeapProperty { min_heap: bool },
    /// Custom invariant expressed as a predicate string
    Custom(String),
}

impl std::fmt::Display for InvariantKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvariantKind::TreeBalance { max_diff } => {
                write!(f, "tree_balance(max_diff={max_diff})")
            }
            InvariantKind::Sorted { descending } => {
                if *descending {
                    write!(f, "sorted(desc)")
                } else {
                    write!(f, "sorted(asc)")
                }
            }
            InvariantKind::Acyclic => write!(f, "acyclic"),
            InvariantKind::BstOrdering => write!(f, "bst_ordering"),
            InvariantKind::HeapProperty { min_heap } => {
                if *min_heap {
                    write!(f, "min_heap")
                } else {
                    write!(f, "max_heap")
                }
            }
            InvariantKind::Custom(pred) => write!(f, "custom({pred})"),
        }
    }
}

/// Error from the structural invariant checker.
pub(crate) type StructuralInvariantError = CheckerError;

/// Checker for recursive structural invariants.
pub(crate) struct StructuralInvariantChecker {
    /// Registered invariants per type
    invariants: HashMap<String, Vec<StructuralInvariant>>,
    /// Known recursive types (type name -> list of recursive field names)
    recursive_types: HashMap<String, Vec<String>>,
}

impl StructuralInvariantChecker {
    pub fn new() -> Self {
        Self {
            invariants: HashMap::new(),
            recursive_types: HashMap::new(),
        }
    }

    /// Register a type as recursive, listing its self-referencing fields.
    pub fn register_recursive_type(&mut self, type_name: String, recursive_fields: Vec<String>) {
        self.recursive_types.insert(type_name, recursive_fields);
    }

    /// Register a structural invariant on a type.
    pub fn register_invariant(&mut self, inv: StructuralInvariant) {
        self.invariants
            .entry(inv.type_name.clone())
            .or_default()
            .push(inv);
    }

    /// Check that a structural invariant is applicable to the type.
    /// - A15001: invariant on non-recursive type
    /// - A15002: tree invariant on non-tree structure
    /// - A15003: sort invariant on non-sequence structure
    pub fn check_invariant_applicability(
        &self,
        type_name: &str,
        kind: &InvariantKind,
        span: &Range<usize>,
    ) -> Vec<StructuralInvariantError> {
        let mut errors = Vec::new();
        if !self.recursive_types.contains_key(type_name) {
            errors.push(StructuralInvariantError {
                code: "A15001".into(),
                message: format!(
                    "structural invariant `{kind}` applied to non-recursive type `{type_name}`"
                ),
                span: span.clone(),
            });
            return errors;
        }

        let fields = &self.recursive_types[type_name];
        match kind {
            InvariantKind::TreeBalance { .. }
            | InvariantKind::BstOrdering
            | InvariantKind::HeapProperty { .. } => {
                // Tree invariants need at least 2 recursive fields (left, right)
                if fields.len() < 2 {
                    errors.push(StructuralInvariantError {
                        code: "A15002".into(),
                        message: format!(
                            "tree invariant `{kind}` requires at least 2 recursive fields, \
                             but `{type_name}` has {}",
                            fields.len()
                        ),
                        span: span.clone(),
                    });
                }
            }
            InvariantKind::Sorted { .. } => {
                // Sort invariant needs exactly 1 recursive field (next pointer)
                if fields.len() != 1 {
                    errors.push(StructuralInvariantError {
                        code: "A15003".into(),
                        message: format!(
                            "sort invariant requires exactly 1 recursive field (next pointer), \
                             but `{type_name}` has {}",
                            fields.len()
                        ),
                        span: span.clone(),
                    });
                }
            }
            InvariantKind::Acyclic | InvariantKind::Custom(_) => {
                // These are valid for any recursive type
            }
        }
        errors
    }

    /// Check that an operation preserves the structural invariant.
    /// - A15004: operation may violate structural invariant
    pub fn check_operation_preserves(
        &self,
        type_name: &str,
        operation: &str,
        modifies_structure: bool,
        has_preservation_proof: bool,
        span: &Range<usize>,
    ) -> Vec<StructuralInvariantError> {
        let mut errors = Vec::new();
        if !modifies_structure {
            return errors; // Read-only operations preserve invariants trivially
        }
        if let Some(invs) = self.invariants.get(type_name) {
            for inv in invs {
                if !has_preservation_proof {
                    errors.push(StructuralInvariantError {
                        code: "A15004".into(),
                        message: format!(
                            "operation `{operation}` modifies `{type_name}` \
                             but has no proof preserving invariant `{}` ({})",
                            inv.name, inv.kind
                        ),
                        span: span.clone(),
                    });
                }
            }
        }
        errors
    }

    /// Get all invariants for a type (test-only).
    #[cfg(test)]
    pub fn get_invariants(&self, type_name: &str) -> Vec<&StructuralInvariant> {
        self.invariants
            .get(type_name)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }
}

impl Default for StructuralInvariantChecker {
    fn default() -> Self {
        Self::new()
    }
}
