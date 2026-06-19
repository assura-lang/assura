//! Native CVC5 encoder session state.

/// Tracks background axioms, string constants, and fresh temporaries.
#[cfg(feature = "cvc5-verify")]
pub(crate) struct Cvc5EncoderState<'a> {
    pub(crate) axioms: Vec<cvc5::Term<'a>>,
    pub(crate) string_constants: Vec<String>,
    pub(crate) fresh_counter: usize,
    pub(crate) use_string_theory: bool,
    field_len_fn: Option<cvc5::Term<'a>>,
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn default_cvc5_encoder_state<'a>() -> Cvc5EncoderState<'a> {
    Cvc5EncoderState {
        axioms: Vec::new(),
        string_constants: Vec::new(),
        fresh_counter: 0,
        use_string_theory: false,
        field_len_fn: None,
    }
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
    len_func
}
