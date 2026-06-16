//! Proc macros for inline Assura contract annotations in Rust.
//!
//! Provides `#[contract]` and `#[trust]` attributes for annotating Rust
//! functions with Assura contracts.
//!
//! - `#[contract]`: In debug builds, generates `debug_assert!` from
//!   `@requires` and `@ensures` clauses in doc comments. In release
//!   builds, no-op (zero runtime cost).
//! - `#[trust("reason")]`: Marks a function as trusted (skip verification).
//!   The reason string is preserved in doc comments for documentation.

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input};

/// Parse `@requires` and `@ensures` clauses from doc comment attributes.
fn extract_clauses(attrs: &[syn::Attribute]) -> (Vec<String>, Vec<String>) {
    let mut requires = Vec::new();
    let mut ensures = Vec::new();
    let mut current_kind: Option<&str> = None;
    let mut current_body = String::new();

    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit_str),
                ..
            }) = &nv.value
        {
            let content = lit_str.value();
            let trimmed = content.trim();

            if let Some(rest) = trimmed.strip_prefix("@requires") {
                // Flush previous clause
                flush_clause(
                    &mut current_kind,
                    &mut current_body,
                    &mut requires,
                    &mut ensures,
                );
                current_kind = Some("requires");
                current_body = rest.trim().to_string();
            } else if let Some(rest) = trimmed.strip_prefix("@ensures") {
                flush_clause(
                    &mut current_kind,
                    &mut current_body,
                    &mut requires,
                    &mut ensures,
                );
                current_kind = Some("ensures");
                current_body = rest.trim().to_string();
            } else if current_kind.is_some() && !trimmed.is_empty() && !trimmed.starts_with('@') {
                // Continuation line
                if !current_body.is_empty() {
                    current_body.push(' ');
                }
                current_body.push_str(trimmed);
            } else if trimmed.is_empty() || trimmed.starts_with('@') {
                // End of multi-line or unrecognized @-clause
                flush_clause(
                    &mut current_kind,
                    &mut current_body,
                    &mut requires,
                    &mut ensures,
                );
            }
        }
    }
    flush_clause(
        &mut current_kind,
        &mut current_body,
        &mut requires,
        &mut ensures,
    );

    (requires, ensures)
}

fn flush_clause(
    kind: &mut Option<&str>,
    body: &mut String,
    requires: &mut Vec<String>,
    ensures: &mut Vec<String>,
) {
    if let Some(k) = kind.take() {
        let text = body.trim().to_string();
        if !text.is_empty() {
            match k {
                "requires" => requires.push(text),
                "ensures" => ensures.push(text),
                _ => {}
            }
        }
        body.clear();
    }
}

/// Replace the standalone word `result` with `__assura_result`, respecting
/// identifier boundaries so that e.g. `partial_result` is left intact.
fn replace_result_word(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let needle = b"result";
    let mut i = 0;

    while i < bytes.len() {
        if i + needle.len() <= bytes.len() && &bytes[i..i + needle.len()] == needle {
            let before_ok =
                i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            let after_ok = i + needle.len() >= bytes.len()
                || !(bytes[i + needle.len()].is_ascii_alphanumeric()
                    || bytes[i + needle.len()] == b'_');
            if before_ok && after_ok {
                out.push_str("__assura_result");
                i += needle.len();
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Mark a function with Assura contract annotations.
///
/// In debug builds, generates `debug_assert!` statements from `@requires`
/// (preconditions) and `@ensures` (postconditions) doc comment clauses.
/// In release builds, the function is unchanged (zero runtime cost).
///
/// # Example
///
/// ```ignore
/// use assura_macros::contract;
///
/// #[contract]
/// /// @requires divisor != 0
/// /// @ensures result == dividend / divisor
/// fn safe_divide(dividend: i64, divisor: i64) -> i64 {
///     dividend / divisor
/// }
/// ```
#[proc_macro_attribute]
pub fn contract(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let (requires, ensures) = extract_clauses(&input.attrs);

    // If no contract clauses found, return the function unchanged
    if requires.is_empty() && ensures.is_empty() {
        return quote!(#input).into();
    }

    let vis = &input.vis;
    let sig = &input.sig;
    let attrs = &input.attrs;
    let block = &input.block;

    // Build precondition assertions
    let pre_asserts: Vec<proc_macro2::TokenStream> = requires
        .iter()
        .map(|pred| {
            let msg = format!("assura: precondition failed: {pred}");
            // Parse the predicate as a Rust expression
            match syn::parse_str::<syn::Expr>(pred) {
                Ok(expr) => quote! {
                    debug_assert!(#expr, #msg);
                },
                Err(_) => {
                    // If we can't parse it, emit a compile-time warning comment
                    let warn_msg =
                        format!("assura: could not parse precondition as Rust expression: {pred}");
                    quote! {
                        // #warn_msg
                        let _ = #warn_msg;
                    }
                }
            }
        })
        .collect();

    // Check if the function has a return type
    let has_return = !matches!(sig.output, syn::ReturnType::Default);

    if has_return && !ensures.is_empty() {
        // Wrap body to capture result and check postconditions
        let post_asserts: Vec<proc_macro2::TokenStream> = ensures
            .iter()
            .map(|pred| {
                let msg = format!("assura: postcondition failed: {pred}");
                // Replace `result` with `__assura_result` using word-boundary
                // awareness so identifiers like `partial_result` are not mangled.
                let adjusted = replace_result_word(pred);
                match syn::parse_str::<syn::Expr>(&adjusted) {
                    Ok(expr) => quote! {
                        debug_assert!(#expr, #msg);
                    },
                    Err(_) => {
                        let warn_msg = format!(
                            "assura: could not parse postcondition as Rust expression: {pred}"
                        );
                        quote! {
                            let _ = #warn_msg;
                        }
                    }
                }
            })
            .collect();

        let stmts = &block.stmts;
        quote! {
            #(#attrs)*
            #vis #sig {
                #(#pre_asserts)*
                let __assura_result = {
                    #(#stmts)*
                };
                #(#post_asserts)*
                __assura_result
            }
        }
        .into()
    } else {
        // No postconditions or void function: just add preconditions
        let stmts = &block.stmts;
        quote! {
            #(#attrs)*
            #vis #sig {
                #(#pre_asserts)*
                #(#stmts)*
            }
        }
        .into()
    }
}

/// Mark a function as trusted, skipping Assura verification.
///
/// The reason string is preserved in a doc comment for documentation.
/// No runtime effect.
///
/// # Example
///
/// ```ignore
/// use assura_macros::trust;
///
/// #[trust("FFI boundary; verified by manual audit")]
/// unsafe fn fast_alloc(n: usize) -> Vec<u8> {
///     vec![0; n]
/// }
/// ```
#[proc_macro_attribute]
pub fn trust(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);

    // Parse the reason string from attribute args
    let reason: String = if !attr.is_empty() {
        let lit = parse_macro_input!(attr as syn::LitStr);
        lit.value()
    } else {
        String::new()
    };

    let vis = &input.vis;
    let sig = &input.sig;
    let attrs = &input.attrs;
    let block = &input.block;

    if reason.is_empty() {
        // No reason given, just pass through
        quote! {
            #(#attrs)*
            #vis #sig #block
        }
        .into()
    } else {
        // Add a doc comment with the trust reason
        let doc = format!(" Trusted: {reason}");
        quote! {
            #[doc = #doc]
            #(#attrs)*
            #vis #sig #block
        }
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_result_word_standalone() {
        assert_eq!(replace_result_word("result > 0"), "__assura_result > 0");
    }

    #[test]
    fn replace_result_word_does_not_mangle_partial_result() {
        assert_eq!(
            replace_result_word("partial_result > 0"),
            "partial_result > 0"
        );
    }

    #[test]
    fn replace_result_word_in_parens() {
        assert_eq!(
            replace_result_word("(result) == 42"),
            "(__assura_result) == 42"
        );
    }

    #[test]
    fn replace_result_word_suffix() {
        assert_eq!(replace_result_word("result_count > 0"), "result_count > 0");
    }

    #[test]
    fn replace_result_word_multiple() {
        assert_eq!(
            replace_result_word("result > 0 && result < 100"),
            "__assura_result > 0 && __assura_result < 100"
        );
    }
}
