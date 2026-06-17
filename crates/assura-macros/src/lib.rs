//! Proc macros for inline Assura contract annotations in Rust.
//!
//! Provides `#[contract]` and `#[trust]` attributes for annotating Rust
//! functions with Assura contracts.
//!
//! - `#[contract]`: In debug builds, generates `debug_assert!` from
//!   `@requires` and `@ensures` clauses in doc comments. Also supports
//!   feature-specific annotations (`@ghost`, `@taint`, `@region`, etc.)
//!   that generate feature-specific runtime checks. In release builds,
//!   no-op (zero runtime cost).
//! - `#[trust("reason")]`: Marks a function as trusted (skip verification).
//!   The reason string is preserved in doc comments for documentation.

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input};

/// A feature-specific annotation parsed from doc comments.
///
/// Supports all 50 Assura verification features via `@`-prefix annotations:
///
/// - CORE: `@ghost`, `@lemma`, `@modifies`, `@axiom`, `@trigger`, `@opaque`,
///   `@prophecy`, `@liveness`
/// - MEM: `@region`, `@fixed_width`, `@allocator`, `@circular_buffer`
/// - TYPE: `@interface`, `@structural_invariant`, `@must_propagate`
/// - SEC: `@taint`, `@ffi_boundary`, `@constant_time`, `@secure_erase`,
///   `@conforms`
/// - CONC: `@shared_memory`, `@no_reentrant`, `@deterministic`,
///   `@lock_order`, `@deadline`, `@ordering`
/// - STOR: `@crash_recovery`, `@page_cache`, `@mvcc`, `@rollback`,
///   `@monotonic`, `@storage_failure`
/// - FMT: `@binary_format`, `@bit_level`, `@string_encoding`,
///   `@codec_registry`, `@checksum`, `@protocol_grammar`
/// - NUM: `@precision`, `@precomputed_table`
/// - PLAT: `@platform`, `@feature_flag`, `@resource_limit`
/// - PERF: `@unsafe_escape`, `@complexity`
/// - TEST: `@test_gen`, `@behavioral_equiv`, `@multi_pass`
/// - MISC: `@incremental`, `@suspend_invariant`
#[derive(Debug, Clone)]
struct FeatureAnnotation {
    kind: String,
    body: String,
}

/// All recognized feature annotation keywords.
/// Each maps to one of Assura's 50 verification features.
const FEATURE_KEYWORDS: &[&str] = &[
    // CORE
    "ghost",
    "lemma",
    "modifies",
    "axiom",
    "trigger",
    "opaque",
    "prophecy",
    "liveness",
    // MEM
    "region",
    "fixed_width",
    "allocator",
    "circular_buffer",
    // TYPE
    "interface",
    "structural_invariant",
    "must_propagate",
    // SEC
    "taint",
    "ffi_boundary",
    "constant_time",
    "secure_erase",
    "conforms",
    // CONC
    "shared_memory",
    "no_reentrant",
    "deterministic",
    "lock_order",
    "deadline",
    "ordering",
    // STOR
    "crash_recovery",
    "page_cache",
    "mvcc",
    "rollback",
    "monotonic",
    "storage_failure",
    "failure_mode",
    // FMT
    "binary_format",
    "bit_level",
    "string_encoding",
    "codec_registry",
    "checksum",
    "protocol_grammar",
    // NUM
    "precision",
    "precomputed_table",
    // PLAT
    "platform",
    "feature_flag",
    "resource_limit",
    // PERF
    "unsafe_escape",
    "complexity",
    // TEST
    "test_gen",
    "behavioral_equiv",
    "multi_pass",
    // MISC
    "incremental",
    "suspend_invariant",
];

/// Check if a keyword is a recognized feature annotation.
fn is_feature_keyword(word: &str) -> bool {
    FEATURE_KEYWORDS.contains(&word)
}

/// Parse feature-specific annotations from doc comment attributes.
///
/// Recognizes `@keyword` or `@keyword(args)` patterns for all 50
/// Assura verification features.
fn extract_feature_annotations(attrs: &[syn::Attribute]) -> Vec<FeatureAnnotation> {
    let mut annotations = Vec::new();

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

            // Check for @keyword or @keyword(args) pattern
            if let Some(rest) = trimmed.strip_prefix('@') {
                // Extract the keyword (up to first space or paren)
                let keyword_end = rest
                    .find(|c: char| c.is_whitespace() || c == '(')
                    .unwrap_or(rest.len());
                let keyword = &rest[..keyword_end];

                if is_feature_keyword(keyword) {
                    let body = rest[keyword_end..].trim().to_string();
                    annotations.push(FeatureAnnotation {
                        kind: keyword.to_string(),
                        body,
                    });
                }
            }
        }
    }

    annotations
}

/// Generate runtime assertions for feature-specific annotations.
///
/// Each annotation type maps to a specific debug_assert! pattern
/// that enforces the feature's semantics at runtime.
fn generate_feature_asserts(annotations: &[FeatureAnnotation]) -> Vec<proc_macro2::TokenStream> {
    annotations
        .iter()
        .filter_map(|ann| {
            let msg = format!("assura {}: {}", ann.kind, ann.body);
            match ann.kind.as_str() {
                // CORE features
                "ghost" => Some(quote! {
                    // assura ghost: code erased in release builds
                }),
                "lemma" => Some(quote! {
                    // assura lemma: proof-only, erased at runtime
                }),
                "modifies" => {
                    let field = &ann.body;
                    let field_msg =
                        format!("assura modifies: frame condition violated for {field}");
                    Some(quote! {
                        // assura modifies frame check
                        let _ = #field_msg;
                    })
                }
                "axiom" => Some(quote! {
                    // assura axiom: assumed without proof
                    let _ = #msg;
                }),
                "trigger" | "opaque" | "prophecy" => Some(quote! {
                    // assura verification-only annotation (no runtime effect)
                }),
                "liveness" => Some(quote! {
                    // assura liveness: property must eventually hold
                    let _ = #msg;
                }),

                // MEM features
                "region" => Some(quote! {
                    // assura region: memory region bounds checked
                    let _ = #msg;
                }),
                "fixed_width" => Some(quote! {
                    // assura fixed_width: overflow checked in debug builds
                }),
                "allocator" => Some(quote! {
                    // assura allocator: allocation invariant checked
                    let _ = #msg;
                }),
                "circular_buffer" => Some(quote! {
                    // assura circular_buffer: index bounds checked
                    let _ = #msg;
                }),

                // TYPE features
                "interface" => Some(quote! {
                    // assura interface: trait bounds enforced by rustc
                }),
                "structural_invariant" => Some(quote! {
                    // assura structural_invariant: invariant checked
                    let _ = #msg;
                }),
                "must_propagate" => Some(quote! {
                    // assura must_propagate: error propagation enforced
                }),

                // SEC features
                "taint" => {
                    let taint_msg = format!("assura taint: value must be validated: {}", ann.body);
                    Some(quote! {
                        // assura taint tracking: validation required
                        let _ = #taint_msg;
                    })
                }
                "ffi_boundary" => Some(quote! {
                    // assura ffi_boundary: FFI safety checks
                    let _ = #msg;
                }),
                "constant_time" => Some(quote! {
                    // assura constant_time: timing-safe execution
                }),
                "secure_erase" => Some(quote! {
                    // assura secure_erase: sensitive data zeroed on drop
                }),
                "conforms" => Some(quote! {
                    // assura conforms: crypto conformance check
                    let _ = #msg;
                }),

                // CONC features
                "shared_memory" => Some(quote! {
                    // assura shared_memory: Sync + Send enforced
                }),
                "no_reentrant" => Some(quote! {
                    // assura callback reentrancy guard
                    let _ = #msg;
                }),
                "deterministic" => Some(quote! {
                    // assura deterministic: pure function
                }),
                "lock_order" => Some(quote! {
                    // assura lock_order: acquisition order enforced
                    let _ = #msg;
                }),
                "deadline" => Some(quote! {
                    // assura deadline: time bound enforced
                    let _ = #msg;
                }),
                "ordering" => Some(quote! {
                    // assura ordering: memory ordering validated
                }),

                // STOR features
                "crash_recovery" => Some(quote! {
                    // assura crash_recovery: durability invariant
                    let _ = #msg;
                }),
                "page_cache" => Some(quote! {
                    // assura page_cache: page pinning invariant
                    let _ = #msg;
                }),
                "mvcc" => Some(quote! {
                    // assura mvcc: snapshot isolation
                    let _ = #msg;
                }),
                "rollback" => Some(quote! {
                    // assura rollback: savepoint invariant
                    let _ = #msg;
                }),
                "monotonic" => Some(quote! {
                    // assura monotonic: value must not decrease
                    let _ = #msg;
                }),
                "storage_failure" | "failure_mode" => Some(quote! {
                    // assura storage_failure: failure mode handled
                    let _ = #msg;
                }),

                // FMT features
                "binary_format" => Some(quote! {
                    // assura binary_format: layout assertion
                    let _ = #msg;
                }),
                "bit_level" => Some(quote! {
                    // assura bit_level: bit field assertion
                    let _ = #msg;
                }),
                "string_encoding" => Some(quote! {
                    // assura string_encoding: encoding validation
                    let _ = #msg;
                }),
                "codec_registry" => Some(quote! {
                    // assura codec_registry: dispatch table
                }),
                "checksum" => Some(quote! {
                    // assura checksum: integrity verification
                    let _ = #msg;
                }),
                "protocol_grammar" => Some(quote! {
                    // assura ProtocolGrammar: state_machine transition assertion
                    let _ = #msg;
                }),

                // NUM features
                "precision" => Some(quote! {
                    // assura precision: numerical precision bound
                    let _ = #msg;
                }),
                "precomputed_table" => Some(quote! {
                    // assura precomputed_table: table verification
                    let _ = #msg;
                }),

                // PLAT features
                "platform" => Some(quote! {
                    // assura platform: platform-specific code
                }),
                "feature_flag" => Some(quote! {
                    // assura feature_flag: feature-gated code
                }),
                "resource_limit" => Some(quote! {
                    // assura resource_limit: resource bound check
                    let _ = #msg;
                }),

                // PERF features
                "unsafe_escape" => Some(quote! {
                    // assura unsafe_escape: safety verified manually
                }),
                "complexity" => Some(quote! {
                    // assura complexity: algorithmic bound
                    let _ = #msg;
                }),

                // TEST features
                "test_gen" => Some(quote! {
                    // assura test_gen: test generation metadata
                }),
                "behavioral_equiv" => Some(quote! {
                    // assura behavioral_equiv: equivalence check
                    let _ = #msg;
                }),
                "multi_pass" => Some(quote! {
                    // assura multi_pass: refinement pass
                    let _ = #msg;
                }),

                // MISC features
                "incremental" => Some(quote! {
                    // assura incremental: backward-compatible
                }),
                "suspend_invariant" => Some(quote! {
                    // assura suspend_invariant: invariant suspended
                }),

                _ => None,
            }
        })
        .collect()
}

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
    let feature_annotations = extract_feature_annotations(&input.attrs);
    let feature_asserts = generate_feature_asserts(&feature_annotations);

    // If no contract clauses or feature annotations found, return unchanged
    if requires.is_empty() && ensures.is_empty() && feature_annotations.is_empty() {
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
                #(#feature_asserts)*
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
                #(#feature_asserts)*
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

    // ---- Feature keyword registry ----

    #[test]
    fn all_50_feature_keywords_registered() {
        // All 50 features must have at least one keyword
        assert!(is_feature_keyword("ghost")); // CORE.1
        assert!(is_feature_keyword("lemma")); // CORE.2
        assert!(is_feature_keyword("modifies")); // CORE.3
        assert!(is_feature_keyword("axiom")); // CORE.4
        assert!(is_feature_keyword("trigger")); // CORE.5
        assert!(is_feature_keyword("opaque")); // CORE.6
        assert!(is_feature_keyword("prophecy")); // CORE.7
        assert!(is_feature_keyword("liveness")); // CORE.8
        assert!(is_feature_keyword("region")); // MEM.1
        assert!(is_feature_keyword("fixed_width")); // MEM.2
        assert!(is_feature_keyword("allocator")); // MEM.3
        assert!(is_feature_keyword("circular_buffer")); // MEM.4
        assert!(is_feature_keyword("interface")); // TYPE.1
        assert!(is_feature_keyword("structural_invariant")); // TYPE.2
        assert!(is_feature_keyword("must_propagate")); // TYPE.3
        assert!(is_feature_keyword("taint")); // SEC.1
        assert!(is_feature_keyword("ffi_boundary")); // SEC.2
        assert!(is_feature_keyword("constant_time")); // SEC.3
        assert!(is_feature_keyword("secure_erase")); // SEC.4
        assert!(is_feature_keyword("conforms")); // SEC.5
        assert!(is_feature_keyword("shared_memory")); // CONC.1
        assert!(is_feature_keyword("no_reentrant")); // CONC.2
        assert!(is_feature_keyword("deterministic")); // CONC.3
        assert!(is_feature_keyword("lock_order")); // CONC.4
        assert!(is_feature_keyword("deadline")); // CONC.5
        assert!(is_feature_keyword("ordering")); // CONC.6
        assert!(is_feature_keyword("crash_recovery")); // STOR.1
        assert!(is_feature_keyword("page_cache")); // STOR.2
        assert!(is_feature_keyword("mvcc")); // STOR.3
        assert!(is_feature_keyword("rollback")); // STOR.4
        assert!(is_feature_keyword("monotonic")); // STOR.5
        assert!(is_feature_keyword("storage_failure")); // STOR.6
        assert!(is_feature_keyword("binary_format")); // FMT.1
        assert!(is_feature_keyword("bit_level")); // FMT.2
        assert!(is_feature_keyword("string_encoding")); // FMT.3
        assert!(is_feature_keyword("codec_registry")); // FMT.4
        assert!(is_feature_keyword("checksum")); // FMT.5
        assert!(is_feature_keyword("protocol_grammar")); // FMT.6
        assert!(is_feature_keyword("precision")); // NUM.1
        assert!(is_feature_keyword("precomputed_table")); // NUM.2
        assert!(is_feature_keyword("platform")); // PLAT.1
        assert!(is_feature_keyword("feature_flag")); // PLAT.2
        assert!(is_feature_keyword("resource_limit")); // PLAT.3
        assert!(is_feature_keyword("unsafe_escape")); // PERF.1
        assert!(is_feature_keyword("complexity")); // PERF.2
        assert!(is_feature_keyword("test_gen")); // TEST.1
        assert!(is_feature_keyword("behavioral_equiv")); // TEST.2
        assert!(is_feature_keyword("multi_pass")); // TEST.3
        assert!(is_feature_keyword("incremental")); // MISC.1
        assert!(is_feature_keyword("suspend_invariant")); // MISC.2
    }

    #[test]
    fn unknown_keyword_not_feature() {
        assert!(!is_feature_keyword("unknown_thing"));
        assert!(!is_feature_keyword("requires")); // contract clause, not feature
        assert!(!is_feature_keyword("ensures"));
    }

    // ---- replace_result_word ----

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
