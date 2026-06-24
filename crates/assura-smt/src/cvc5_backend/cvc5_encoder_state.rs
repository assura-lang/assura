//! Native CVC5 encoder session state.

use std::collections::HashMap;

use crate::cvc5_adt::{Cvc5AdtDef, Cvc5AdtNativeSymbols};

/// Tracks background axioms, string constants, and fresh temporaries.
#[cfg(feature = "cvc5-verify")]
pub(crate) struct Cvc5EncoderState<'a> {
    pub(crate) axioms: Vec<cvc5::Term<'a>>,
    pub(crate) string_constants: Vec<String>,
    pub(crate) fresh_counter: usize,
    pub(crate) use_string_theory: bool,
    field_len_fn: Option<cvc5::Term<'a>>,
    /// Cached uninterpreted function symbols (`name@arity@bool|int`) so axioms
    /// from requires share the same UF with ensures (CVC5 does not intern by name).
    uf_cache: HashMap<String, cvc5::Term<'a>>,
    pub(crate) struct_adt_symbols: HashMap<String, Cvc5AdtNativeSymbols<'a>>,
    pub(crate) struct_adt_defs: HashMap<String, Cvc5AdtDef>,
    /// Contract-level quantifier trigger manager (seeded from clauses, refined during encode).
    pub(crate) trigger_manager: crate::advanced::TriggerManager,
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn default_cvc5_encoder_state<'a>() -> Cvc5EncoderState<'a> {
    Cvc5EncoderState {
        axioms: Vec::new(),
        string_constants: Vec::new(),
        fresh_counter: 0,
        use_string_theory: false,
        field_len_fn: None,
        uf_cache: HashMap::new(),
        struct_adt_symbols: HashMap::new(),
        struct_adt_defs: HashMap::new(),
        trigger_manager: crate::advanced::TriggerManager::new(),
    }
}

/// Seed the encoder's trigger manager from all contract clauses (requires/ensures/etc.).
///
/// Delegates to [`crate::trigger_seed_policy`] (shared with Z3).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn seed_cvc5_trigger_manager_from_clauses(
    state: &mut Cvc5EncoderState<'_>,
    clauses: &[assura_ast::Clause],
) {
    crate::trigger_seed_policy::seed_trigger_manager_from_clauses(
        clauses,
        &mut state.trigger_manager,
    );
}

/// Register Call/MethodCall names from an expression tree for quantifier e-matching.
///
/// Delegates to [`crate::trigger_seed_policy`].
pub(crate) fn register_trigger_functions_from_expr(
    expr: &assura_ast::SpExpr,
    tm: &mut crate::advanced::TriggerManager,
) {
    crate::trigger_seed_policy::register_trigger_functions_from_expr(expr, tm);
}

/// Canonical length variable for a named binding (`__canonical_len_{name}`).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn canonical_length_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    name: &str,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    let key = crate::encode_atom_policy::canonical_length_name(name);
    if let Some(v) = vars.get(&key) {
        return v.clone();
    }
    let v = tm.mk_const(tm.integer_sort(), &key);
    let zero = tm.mk_integer(0);
    state
        .axioms
        .push(tm.mk_term(cvc5::Kind::Geq, &[v.clone(), zero]));
    vars.insert(key, v.clone());
    v
}

/// Native CVC5 quantifier encoding session (term manager + var map + state).
///
/// `vars` / `state` borrows are independent of the term lifetime `'a` so
/// `encode_expr_cvc5` can pass its normal `&mut` parameters without E0621.
#[cfg(feature = "cvc5-verify")]
pub(crate) struct Cvc5QuantifierEncodeCtx<'a, 'v, 's> {
    pub tm: &'a cvc5::TermManager,
    pub vars: &'v mut std::collections::HashMap<String, cvc5::Term<'a>>,
    pub state: &'s mut Cvc5EncoderState<'a>,
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn field_len_fn_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    if let Some(f) = state.field_len_fn.as_ref() {
        return f.clone();
    }
    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
    let len_func = tm.mk_const(len_sort, "__field_len");
    state.field_len_fn = Some(len_func.clone());
    // Keep uf_cache aligned so `collection_len_of("__field_len")` reuses this symbol.
    state
        .uf_cache
        .insert(uf_cache_key("__field_len", 1, false), len_func.clone());
    len_func
}

/// Cache key for uninterpreted functions (name, arity, returns_bool).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn uf_cache_key(name: &str, arity: usize, returns_bool: bool) -> String {
    let sort_tag = if returns_bool { "b" } else { "i" };
    format!("{name}@{arity}@{sort_tag}")
}

/// Interned UF symbol for the encoder session (avoids per-call `mk_const` divergence).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn intern_uf_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
    name: &str,
    arity: usize,
    returns_bool: bool,
) -> cvc5::Term<'a> {
    let key = uf_cache_key(name, arity, returns_bool);
    if let Some(f) = state.uf_cache.get(&key) {
        return f.clone();
    }
    let domain: Vec<cvc5::Sort> = (0..arity).map(|_| tm.integer_sort()).collect();
    let codomain = if returns_bool {
        tm.boolean_sort()
    } else {
        tm.integer_sort()
    };
    let func_sort = tm.mk_fun_sort(&domain, codomain);
    let func_const = tm.mk_const(func_sort, name);
    if name == "__field_len" && arity == 1 && !returns_bool {
        state.field_len_fn = Some(func_const.clone());
    }
    state.uf_cache.insert(key, func_const.clone());
    func_const
}
