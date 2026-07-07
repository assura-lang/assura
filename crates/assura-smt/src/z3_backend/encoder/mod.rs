//! Z3 expression encoder: translates Assura AST expressions into Z3 formulas.
//!
//! Split by expression category:
//! - [`value`] — `Z3Value`
//! - [`core_impl`] — `Encoder` construction, shared helpers (fresh vars, UFs, lengths, literals)
//! - [`methods`] — `encode_expr` dispatch, raw tokens, binops
//! - [`adt`] — ADT emulation (define/construct/test/access), match-pattern encoding
//! - [`calls`] — function/method call, field access, index encoding
//! - [`quantifier`] — forall/exists domain guards, trigger pattern inference
//! - [`unmodelable`] — unmodelable-feature detection / reasons
//! - [`bitvector`] — fixed-width BV helpers (`BitvectorEncoder`)
//!
//! Contains the `Encoder` struct, Z3 value wrapper types, raw-token parsing,
//! and unmodelable-feature detection.

mod adt;
mod bitvector;
mod calls;
mod core_impl;
mod encode_term_impl;
mod methods;
mod quantifier;
mod unmodelable;
mod value;

use std::collections::HashMap;
use std::sync::Once;
use z3::ast;

// Re-exports for `z3_backend` / tests (`crate::z3_backend::encoder::…`).
// Some symbols are only referenced outside this module; allow unused here.
#[allow(unused_imports)]
pub(crate) use bitvector::{BitvectorEncoder, OverflowResult};
#[allow(unused_imports)]
pub(crate) use unmodelable::{collect_unmodelable_reasons, expr_has_unmodelable_features};
#[allow(unused_imports)]
pub(crate) use value::Z3Value;

static BITVECTOR_API_WIRED: Once = Once::new();

// -----------------------------------------------------------------------
// Expression encoder types
// -----------------------------------------------------------------------

/// A single constructor in an ADT emulation.
///
/// Each constructor has a unique integer tag, a name, and a list of named
/// accessor fields. The encoder uses uninterpreted functions for accessors
/// and integer equality for constructor testing.
#[derive(Debug, Clone)]
pub(crate) struct AdtConstructor {
    /// Display name of the constructor (e.g., "Some", "None").
    pub(crate) name: String,
    /// Unique integer tag assigned to this constructor.
    pub(crate) tag: i64,
    /// Named accessor fields. Each accessor is encoded as an uninterpreted
    /// function `Int -> Int` applied to the ADT value.
    pub(crate) accessors: Vec<String>,
}

/// An ADT (algebraic data type) definition emulated via integer tags and
/// uninterpreted functions.
///
/// Since the encoder is currently untyped (all values are Int or Bool),
/// ADTs are emulated as follows:
/// - Each constructor gets a unique integer tag.
/// - A tag function (`__adt_tag_<name>`) returns the constructor tag.
/// - Each accessor is an uninterpreted function `__adt_<adt>_<accessor>(x) -> Int`.
/// - Constructor tester: `tag(x) == CONSTRUCTOR_TAG`.
/// - Injectivity axiom: `Ctor(a, b) == Ctor(c, d) => a == c && b == d`.
/// - Exhaustiveness: `tag(x) == Tag1 || tag(x) == Tag2 || ...`.
#[derive(Debug, Clone)]
pub(crate) struct AdtDef {
    /// ADT type name (e.g., "Option", "List").
    pub(crate) name: String,
    /// Constructors in definition order.
    pub(crate) constructors: Vec<AdtConstructor>,
}

/// Translates Assura AST expressions into Z3 formulas.
pub(crate) struct Encoder {
    pub(crate) vars: HashMap<String, Z3Value>,
    /// Tracks known function arities for uninterpreted function encoding
    pub(crate) func_arities: HashMap<String, usize>,
    pub(crate) fresh_counter: u32,
    /// Background axioms collected during encoding (e.g., len >= 0).
    /// These are asserted into the solver before each verification check.
    pub(crate) background_axioms: Vec<z3::ast::Bool>,
    /// Trigger manager for quantifier e-matching hints
    pub(crate) trigger_manager: crate::advanced::TriggerManager,
    /// Track distinct string constant names for pairwise distinctness axioms
    pub(crate) string_constants: Vec<String>,
    /// When true, use native Z3 string theory (QF_S/QF_SLIA) for string
    /// literals and .length(). When false (default), use integer encoding.
    pub(crate) use_string_theory: bool,
    /// Registered ADT definitions for lightweight emulation.
    pub(crate) adt_defs: HashMap<String, AdtDef>,
    /// Fixed-width params: name -> signed? (false = unsigned).
    pub(crate) bv_signed: HashMap<String, bool>,
    /// Canonical `.length()` variables per identifier (#267).
    canonical_lengths: HashMap<String, ast::Int>,
    /// Same-file pure callees with `ensures { result == <expr> }` for equating
    /// ensures-side calls to functional bodies (not free UFs).
    pub(crate) callee_specs: HashMap<String, crate::encode_callee_policy::CalleeFunctionalSpec>,
}
