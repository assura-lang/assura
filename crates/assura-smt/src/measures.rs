//! Measure definitions (T054): mathematical measure functions used in contracts.
//!
//! Measures like `len`, `elems`, `keys` are encoded as uninterpreted
//! functions in Z3 with standard axioms constraining their behavior.

/// The sort (type) of a measure parameter or return value in the SMT encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeasureSort {
    /// Non-negative integer (used for len, size).
    Nat,
    /// Uninterpreted set sort (used for elems, keys, values).
    Set,
    /// Uninterpreted collection sort (parameter type for most measures).
    Collection,
    /// Uninterpreted map sort (parameter type for keys/values).
    Map,
}

/// An axiom attached to a measure definition.
///
/// Each axiom is a universally quantified property that the SMT solver
/// can use when reasoning about the measure. For example, `len(xs) >= 0`
/// or `len(empty) == 0`.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasureAxiom {
    /// Human-readable description of the axiom.
    pub description: String,
    /// The axiom tag used to select which axioms to assert.
    pub tag: MeasureAxiomTag,
}

/// Tags for built-in measure axioms, used by the Z3 encoder to generate
/// the correct Z3 assertions for each axiom.
#[derive(Debug, Clone, PartialEq)]
pub enum MeasureAxiomTag {
    /// `measure(x) >= 0` (non-negativity).
    NonNegative,
    /// `measure(empty) == 0`.
    EmptyIsZero,
    /// `measure(append(xs, x)) == measure(xs) + 1`.
    AppendIncrement,
    /// `measure_a(xs) == measure_b(xs)` (e.g., size == len for lists).
    EquivalentTo(String),
    /// `measure(empty_map) == empty_set`.
    EmptyMapEmptySet,
    /// Custom axiom with a textual description.
    Custom(String),
}

/// Definition of a mathematical measure function used in contracts.
///
/// Measures like `len`, `elems`, `keys` are encoded as uninterpreted
/// functions in Z3 with standard axioms constraining their behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasureDefinition {
    /// Name of the measure (e.g., "len", "elems").
    pub name: String,
    /// Parameter sorts.
    pub param_sorts: Vec<MeasureSort>,
    /// Return sort.
    pub return_sort: MeasureSort,
    /// Axioms constraining the measure.
    pub axioms: Vec<MeasureAxiom>,
}

impl MeasureDefinition {
    /// Create a new measure definition.
    pub fn new(
        name: impl Into<String>,
        param_sorts: Vec<MeasureSort>,
        return_sort: MeasureSort,
    ) -> Self {
        Self {
            name: name.into(),
            param_sorts,
            return_sort,
            axioms: Vec::new(),
        }
    }

    /// Add an axiom to this measure.
    pub fn with_axiom(mut self, description: impl Into<String>, tag: MeasureAxiomTag) -> Self {
        self.axioms.push(MeasureAxiom {
            description: description.into(),
            tag,
        });
        self
    }

    /// Returns true if this measure returns a Nat (non-negative integer).
    pub fn returns_nat(&self) -> bool {
        self.return_sort == MeasureSort::Nat
    }
}

/// Register the five built-in measures with their standard axioms.
///
/// Built-in measures:
/// - `len(collection) -> Nat`: length of a list/array/string
/// - `elems(collection) -> Set`: elements of a list/set
/// - `keys(map) -> Set`: keys of a map
/// - `values(map) -> Set`: values of a map
/// - `size(collection) -> Nat`: cardinality/size
pub fn register_builtin_measures() -> Vec<MeasureDefinition> {
    vec![
        // len(collection) -> Nat
        MeasureDefinition::new("len", vec![MeasureSort::Collection], MeasureSort::Nat)
            .with_axiom("len(xs) >= 0", MeasureAxiomTag::NonNegative)
            .with_axiom("len(empty) == 0", MeasureAxiomTag::EmptyIsZero)
            .with_axiom(
                "len(append(xs, x)) == len(xs) + 1",
                MeasureAxiomTag::AppendIncrement,
            ),
        // elems(collection) -> Set
        MeasureDefinition::new("elems", vec![MeasureSort::Collection], MeasureSort::Set)
            .with_axiom("elems(empty) == empty_set", MeasureAxiomTag::EmptyIsZero),
        // keys(map) -> Set
        MeasureDefinition::new("keys", vec![MeasureSort::Map], MeasureSort::Set).with_axiom(
            "keys(empty_map) == empty_set",
            MeasureAxiomTag::EmptyMapEmptySet,
        ),
        // values(map) -> Set
        MeasureDefinition::new("values", vec![MeasureSort::Map], MeasureSort::Set).with_axiom(
            "values(empty_map) == empty_set",
            MeasureAxiomTag::EmptyMapEmptySet,
        ),
        // size(collection) -> Nat
        MeasureDefinition::new("size", vec![MeasureSort::Collection], MeasureSort::Nat)
            .with_axiom("size(xs) >= 0", MeasureAxiomTag::NonNegative)
            .with_axiom("size(empty) == 0", MeasureAxiomTag::EmptyIsZero)
            .with_axiom(
                "size(xs) == len(xs) for lists",
                MeasureAxiomTag::EquivalentTo("len".into()),
            ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_new_basic() {
        let m = MeasureDefinition::new("len", vec![MeasureSort::Collection], MeasureSort::Nat);
        assert_eq!(m.name, "len");
        assert_eq!(m.param_sorts, vec![MeasureSort::Collection]);
        assert_eq!(m.return_sort, MeasureSort::Nat);
        assert!(m.axioms.is_empty());
    }

    #[test]
    fn measure_with_axiom_chains() {
        let m = MeasureDefinition::new("len", vec![MeasureSort::Collection], MeasureSort::Nat)
            .with_axiom("len >= 0", MeasureAxiomTag::NonNegative)
            .with_axiom("len(empty) == 0", MeasureAxiomTag::EmptyIsZero);
        assert_eq!(m.axioms.len(), 2);
        assert_eq!(m.axioms[0].tag, MeasureAxiomTag::NonNegative);
        assert_eq!(m.axioms[1].tag, MeasureAxiomTag::EmptyIsZero);
    }

    #[test]
    fn measure_returns_nat_true() {
        let m = MeasureDefinition::new("len", vec![MeasureSort::Collection], MeasureSort::Nat);
        assert!(m.returns_nat());
    }

    #[test]
    fn measure_returns_nat_false() {
        let m = MeasureDefinition::new("elems", vec![MeasureSort::Collection], MeasureSort::Set);
        assert!(!m.returns_nat());
    }

    #[test]
    fn builtin_measures_count() {
        let measures = register_builtin_measures();
        assert_eq!(measures.len(), 5);
    }

    #[test]
    fn builtin_len_has_three_axioms() {
        let measures = register_builtin_measures();
        let len = measures.iter().find(|m| m.name == "len").unwrap();
        assert_eq!(len.axioms.len(), 3);
        assert!(len.returns_nat());
    }

    #[test]
    fn builtin_elems_returns_set() {
        let measures = register_builtin_measures();
        let elems = measures.iter().find(|m| m.name == "elems").unwrap();
        assert_eq!(elems.return_sort, MeasureSort::Set);
        assert!(!elems.returns_nat());
    }

    #[test]
    fn builtin_keys_takes_map() {
        let measures = register_builtin_measures();
        let keys = measures.iter().find(|m| m.name == "keys").unwrap();
        assert_eq!(keys.param_sorts, vec![MeasureSort::Map]);
    }

    #[test]
    fn builtin_values_takes_map() {
        let measures = register_builtin_measures();
        let values = measures.iter().find(|m| m.name == "values").unwrap();
        assert_eq!(values.param_sorts, vec![MeasureSort::Map]);
    }

    #[test]
    fn builtin_size_equivalent_to_len() {
        let measures = register_builtin_measures();
        let size = measures.iter().find(|m| m.name == "size").unwrap();
        let has_equiv = size
            .axioms
            .iter()
            .any(|a| matches!(&a.tag, MeasureAxiomTag::EquivalentTo(s) if s == "len"));
        assert!(has_equiv);
    }

    #[test]
    fn builtin_names() {
        let measures = register_builtin_measures();
        let names: Vec<&str> = measures.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["len", "elems", "keys", "values", "size"]);
    }

    #[test]
    fn measure_sort_equality() {
        assert_eq!(MeasureSort::Nat, MeasureSort::Nat);
        assert_ne!(MeasureSort::Nat, MeasureSort::Set);
        assert_ne!(MeasureSort::Collection, MeasureSort::Map);
    }

    #[test]
    fn axiom_tag_custom() {
        let m = MeasureDefinition::new("custom", vec![], MeasureSort::Nat)
            .with_axiom("custom axiom", MeasureAxiomTag::Custom("special".into()));
        assert_eq!(m.axioms[0].tag, MeasureAxiomTag::Custom("special".into()));
    }
}
