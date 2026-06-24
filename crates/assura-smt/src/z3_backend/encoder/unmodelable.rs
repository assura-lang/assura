//! Unmodelable-feature detection for SMT clauses (Z3 encoder facade).
//!
//! Implementation lives in [`crate::unmodelable`] (shared with CVC5). This module
//! re-exports the shared API so `crate::z3_backend::encoder::…` import paths stay stable.

pub(crate) use crate::unmodelable::{
    collect_unmodelable_reasons, expr_has_unmodelable_features, field_chain_depth,
    flatten_field_chain, has_deep_field_chain, is_self_rooted,
};
