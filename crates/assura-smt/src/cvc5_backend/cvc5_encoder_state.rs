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
#[cfg(feature = "cvc5-verify")]
pub(crate) fn seed_cvc5_trigger_manager_from_clauses(
    state: &mut Cvc5EncoderState<'_>,
    clauses: &[assura_ast::Clause],
) {
    for clause in clauses {
        register_trigger_functions_from_expr(&clause.body, &mut state.trigger_manager);
    }
}

/// Register Call/MethodCall names from an expression tree for quantifier e-matching.
pub(crate) fn register_trigger_functions_from_expr(
    expr: &assura_ast::SpExpr,
    tm: &mut crate::advanced::TriggerManager,
) {
    use assura_ast::Expr;
    match &expr.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node {
                tm.register_function(name.clone());
            }
            for a in args {
                register_trigger_functions_from_expr(a, tm);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            tm.register_function(method.clone());
            register_trigger_functions_from_expr(receiver, tm);
            for a in args {
                register_trigger_functions_from_expr(a, tm);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            register_trigger_functions_from_expr(lhs, tm);
            register_trigger_functions_from_expr(rhs, tm);
        }
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) | Expr::Ghost(inner) => {
            register_trigger_functions_from_expr(inner, tm);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            register_trigger_functions_from_expr(cond, tm);
            register_trigger_functions_from_expr(then_branch, tm);
            if let Some(eb) = else_branch {
                register_trigger_functions_from_expr(eb, tm);
            }
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            register_trigger_functions_from_expr(domain, tm);
            register_trigger_functions_from_expr(body, tm);
        }
        Expr::Index { expr: e, index } => {
            register_trigger_functions_from_expr(e, tm);
            register_trigger_functions_from_expr(index, tm);
        }
        Expr::Field(obj, _) => register_trigger_functions_from_expr(obj, tm),
        Expr::Block(items) | Expr::Tuple(items) | Expr::List(items) => {
            for e in items {
                register_trigger_functions_from_expr(e, tm);
            }
        }
        Expr::Apply { args, .. } => {
            for a in args {
                register_trigger_functions_from_expr(a, tm);
            }
        }
        Expr::Let { value, body, .. } => {
            register_trigger_functions_from_expr(value, tm);
            register_trigger_functions_from_expr(body, tm);
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            register_trigger_functions_from_expr(scrutinee, tm);
            for arm in arms {
                register_trigger_functions_from_expr(&arm.body, tm);
            }
        }
        Expr::Cast { expr: inner, .. } => register_trigger_functions_from_expr(inner, tm),
        _ => {}
    }
}

/// Canonical length variable for a named binding (`__canonical_len_{name}`).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn canonical_length_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    name: &str,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    let key = format!("__canonical_len_{name}");
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
#[cfg(feature = "cvc5-verify")]
pub(crate) struct Cvc5QuantifierEncodeCtx<'a> {
    pub tm: &'a cvc5::TermManager,
    pub vars: &'a mut std::collections::HashMap<String, cvc5::Term<'a>>,
    pub state: &'a mut Cvc5EncoderState<'a>,
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
