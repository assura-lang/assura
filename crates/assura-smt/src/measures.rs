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
