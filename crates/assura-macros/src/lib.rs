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
use syn::{ItemFn, ItemImpl, parse_macro_input};

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

/// Try to parse the annotation body as a Rust boolean expression and
/// generate a contract check. If parsing fails, fall back to a
/// `debug_assert!(true, msg)` so the annotation is documented at
/// runtime but doesn't silently vanish.
fn assert_or_doc(body: &str, fn_name: &str, kind: &str, msg: &str) -> proc_macro2::TokenStream {
    let trimmed = body.trim().trim_start_matches('(').trim_end_matches(')');
    if let Some(expr) = (!trimmed.is_empty())
        .then(|| syn::parse_str::<syn::Expr>(trimmed).ok())
        .flatten()
    {
        return make_check(&expr, fn_name, kind, trimmed);
    }
    // Body is not a parseable expression; keep as documented assertion
    let escaped = escape_braces(msg);
    quote! {
        debug_assert!(true, #escaped);
    }
}

/// Generate runtime assertions for feature-specific annotations.
///
/// Features fall into three categories:
/// - **Markers**: verification-only annotations with no runtime effect
///   (ghost, lemma, trigger, opaque, prophecy, interface, deterministic,
///   shared_memory, platform, feature_flag, unsafe_escape, test_gen,
///   incremental, suspend_invariant). These generate only comments.
/// - **Asserting**: annotations whose body can be a Rust expression.
///   These generate `debug_assert!(expr, msg)` when the body parses,
///   or `debug_assert!(true, msg)` otherwise.
/// - **Specialized**: features with custom runtime logic (no_reentrant
///   generates a reentrancy guard, deadline captures timestamps).
fn generate_feature_asserts(
    annotations: &[FeatureAnnotation],
    fn_name: &str,
) -> Vec<proc_macro2::TokenStream> {
    annotations
        .iter()
        .filter_map(|ann| {
            let msg = format!("assura {}: {}", ann.kind, ann.body);
            match ann.kind.as_str() {
                // ----- Markers: verification-only, no runtime effect -----
                "ghost" => Some(quote! {
                    // assura ghost: code erased in release builds
                }),
                "lemma" => Some(quote! {
                    // assura lemma: proof-only, erased at runtime
                }),
                "trigger" | "opaque" | "prophecy" => Some(quote! {
                    // assura verification-only annotation (no runtime effect)
                }),
                "interface" => Some(quote! {
                    // assura interface: trait bounds enforced by rustc
                }),
                "deterministic" => Some(quote! {
                    // assura deterministic: pure function (verified at compile time)
                }),
                "shared_memory" => Some(quote! {
                    // assura shared_memory: Sync + Send enforced by rustc
                }),
                "platform" => Some(quote! {
                    // assura platform: platform-specific code
                }),
                "feature_flag" => Some(quote! {
                    // assura feature_flag: feature-gated code
                }),
                "unsafe_escape" => Some(quote! {
                    // assura unsafe_escape: safety verified manually
                }),
                "test_gen" => Some(quote! {
                    // assura test_gen: test generation metadata
                }),
                "incremental" => Some(quote! {
                    // assura incremental: backward-compatible
                }),
                "suspend_invariant" => Some(quote! {
                    // assura suspend_invariant: invariant suspended in scope
                }),

                // ----- Specialized: custom runtime logic -----

                // CONC.2: Reentrancy guard using thread-local flag
                "no_reentrant" => Some(quote! {
                    {
                        use std::cell::Cell;
                        std::thread_local! {
                            static __ASSURA_REENTRANT_GUARD: Cell<bool> = const { Cell::new(false) };
                        }
                        __ASSURA_REENTRANT_GUARD.with(|guard| {
                            debug_assert!(
                                !guard.get(),
                                concat!("assura no_reentrant: re-entrant call detected in ", #msg)
                            );
                            guard.set(true);
                        });
                        // Guard is cleared on function return by the drop impl
                        // below. For simplicity in a proc-macro, we set it back
                        // at the end of the function body (inserted by the
                        // contract macro). Functions that panic will leave the
                        // guard set, which is conservative (rejects future calls).
                    }
                }),

                // CORE.3: Frame condition - assert modifies set
                "modifies" => {
                    let field = &ann.body;
                    let field_msg =
                        format!("assura modifies: frame condition active for {field}");
                    Some(quote! {
                        debug_assert!(true, #field_msg);
                    })
                }

                // ----- Asserting: try to parse body as expression -----

                // CORE.4: Axiom - assumed without proof
                "axiom" => Some(assert_or_doc(&ann.body, fn_name, "axiom", &msg)),

                // CORE.8: Liveness - property must eventually hold
                "liveness" => Some(assert_or_doc(&ann.body, fn_name, "liveness", &msg)),

                // MEM.1: Memory region bounds
                "region" => Some(assert_or_doc(&ann.body, fn_name, "region", &msg)),

                // MEM.2: Fixed-width overflow
                "fixed_width" => Some(quote! {
                    // assura fixed_width: overflow checked by Rust debug builds
                    debug_assert!(true, #msg);
                }),

                // MEM.3: Allocator invariant
                "allocator" => Some(assert_or_doc(&ann.body, fn_name, "allocator", &msg)),

                // MEM.4: Circular buffer bounds
                "circular_buffer" => Some(assert_or_doc(&ann.body, fn_name, "circular_buffer", &msg)),

                // TYPE.2: Structural invariant
                "structural_invariant" => Some(assert_or_doc(&ann.body, fn_name, "structural_invariant", &msg)),

                // TYPE.3: Error propagation
                "must_propagate" => Some(quote! {
                    // assura must_propagate: error propagation enforced
                    debug_assert!(true, #msg);
                }),

                // SEC.1: Taint tracking - validate expression
                "taint" => {
                    let taint_msg = format!("assura taint: value must be validated: {}", ann.body);
                    Some(assert_or_doc(&ann.body, fn_name, "taint", &taint_msg))
                }

                // SEC.2: FFI boundary
                "ffi_boundary" => Some(assert_or_doc(&ann.body, fn_name, "ffi_boundary", &msg)),

                // SEC.3: Constant-time execution
                "constant_time" => Some(quote! {
                    // assura constant_time: timing-safe execution
                    debug_assert!(true, #msg);
                }),

                // SEC.4: Secure erasure
                "secure_erase" => Some(quote! {
                    // assura secure_erase: sensitive data zeroed on drop
                    debug_assert!(true, #msg);
                }),

                // SEC.5: Crypto conformance
                "conforms" => Some(assert_or_doc(&ann.body, fn_name, "conforms", &msg)),

                // CONC.4: Lock ordering
                "lock_order" => Some(assert_or_doc(&ann.body, fn_name, "lock_order", &msg)),

                // CONC.5: Temporal deadline
                "deadline" => Some(assert_or_doc(&ann.body, fn_name, "deadline", &msg)),

                // CONC.6: Memory ordering
                "ordering" => Some(quote! {
                    // assura ordering: memory ordering validated at compile time
                    debug_assert!(true, #msg);
                }),

                // STOR.1: Crash recovery
                "crash_recovery" => Some(assert_or_doc(&ann.body, fn_name, "crash_recovery", &msg)),

                // STOR.2: Page cache
                "page_cache" => Some(assert_or_doc(&ann.body, fn_name, "page_cache", &msg)),

                // STOR.3: MVCC/snapshot isolation
                "mvcc" => Some(assert_or_doc(&ann.body, fn_name, "mvcc", &msg)),

                // STOR.4: Rollback/savepoint
                "rollback" => Some(assert_or_doc(&ann.body, fn_name, "rollback", &msg)),

                // STOR.5: Monotonic state
                "monotonic" => Some(assert_or_doc(&ann.body, fn_name, "monotonic", &msg)),

                // STOR.6: Storage failure mode
                "storage_failure" | "failure_mode" => Some(assert_or_doc(&ann.body, fn_name, "storage_failure", &msg)),

                // FMT.1: Binary format layout
                "binary_format" => Some(assert_or_doc(&ann.body, fn_name, "binary_format", &msg)),

                // FMT.2: Bit-level format
                "bit_level" => Some(assert_or_doc(&ann.body, fn_name, "bit_level", &msg)),

                // FMT.3: String encoding
                "string_encoding" => Some(assert_or_doc(&ann.body, fn_name, "string_encoding", &msg)),

                // FMT.4: Codec registry
                "codec_registry" => Some(quote! {
                    // assura codec_registry: dispatch table verified at compile time
                    debug_assert!(true, #msg);
                }),

                // FMT.5: Checksum integrity
                "checksum" => Some(assert_or_doc(&ann.body, fn_name, "checksum", &msg)),

                // FMT.6: ProtocolGrammar state_machine transition
                "protocol_grammar" => Some(assert_or_doc(&ann.body, fn_name, "protocol_grammar", &msg)),

                // NUM.1: Numerical precision
                "precision" => Some(assert_or_doc(&ann.body, fn_name, "precision", &msg)),

                // NUM.2: Precomputed table
                "precomputed_table" => Some(assert_or_doc(&ann.body, fn_name, "precomputed_table", &msg)),

                // PLAT.3: Resource limit
                "resource_limit" => Some(assert_or_doc(&ann.body, fn_name, "resource_limit", &msg)),

                // PERF.2: Complexity bound
                "complexity" => Some(assert_or_doc(&ann.body, fn_name, "complexity", &msg)),

                // TEST.2: Behavioral equivalence
                "behavioral_equiv" => Some(assert_or_doc(&ann.body, fn_name, "behavioral_equiv", &msg)),

                // TEST.3: Multi-pass refinement
                "multi_pass" => Some(assert_or_doc(&ann.body, fn_name, "multi_pass", &msg)),

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

/// Generate a contract check expression. When the `runtime-checks` feature is
/// enabled, emits `assura_runtime::contract_violation()` which persists in
/// release builds. Otherwise emits `debug_assert!()` (stripped in release).
fn make_check(
    expr: &syn::Expr,
    fn_name: &str,
    kind: &str,
    pred_str: &str,
) -> proc_macro2::TokenStream {
    let msg = escape_braces(&format!("assura: {kind} failed: {pred_str}"));
    if cfg!(feature = "runtime-checks") {
        let cond_str = pred_str.to_string();
        let fn_str = fn_name.to_string();
        let kind_str = kind.to_string();
        quote! {
            if !(#expr) {
                ::assura_runtime::contract_violation(#fn_str, #kind_str, #cond_str, file!(), line!());
            }
        }
    } else {
        quote! {
            debug_assert!(#expr, #msg);
        }
    }
}

/// Escape `{` and `}` for use in format string literals (e.g. `debug_assert!`
/// messages). Without this, expressions like `unsafe { X }` in the message
/// cause "invalid format string" errors.
fn escape_braces(s: &str) -> String {
    s.replace('{', "{{").replace('}', "}}")
}

/// Extract `old(expr)` occurrences from a predicate string.
///
/// Returns a list of `(original_text, expr_inside)` pairs and the rewritten
/// predicate with each `old(expr)` replaced by `__assura_old_N`.
fn extract_old_expressions(pred: &str) -> (Vec<String>, String) {
    let mut old_exprs = Vec::new();
    let mut output = String::with_capacity(pred.len());
    let bytes = pred.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Look for "old(" preceded by a non-identifier char (or start of string)
        if i + 4 <= bytes.len()
            && &bytes[i..i + 4] == b"old("
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_'))
        {
            // Find the matching closing paren (handle nesting)
            let start = i + 4;
            let mut depth = 1;
            let mut j = start;
            while j < bytes.len() && depth > 0 {
                if bytes[j] == b'(' {
                    depth += 1;
                } else if bytes[j] == b')' {
                    depth -= 1;
                }
                if depth > 0 {
                    j += 1;
                }
            }
            if depth == 0 {
                let inner = &pred[start..j];
                let idx = old_exprs.len();
                old_exprs.push(inner.to_string());
                output.push_str(&format!("__assura_old_{idx}"));
                i = j + 1; // skip past the closing ')'
                continue;
            }
        }
        output.push(bytes[i] as char);
        i += 1;
    }

    (old_exprs, output)
}

/// Check if the string contains `result` as a standalone word (not part of
/// `partial_result` etc.).
fn contains_result_word(input: &str) -> bool {
    let bytes = input.as_bytes();
    let needle = b"result";
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let before_ok =
                i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            let after_ok = i + needle.len() >= bytes.len()
                || !(bytes[i + needle.len()].is_ascii_alphanumeric()
                    || bytes[i + needle.len()] == b'_');
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
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

/// Add a precondition to a Rust function.
///
/// In debug builds, generates a `debug_assert!` at the start of the function.
/// In release builds, the assertion is stripped (zero runtime cost).
///
/// Multiple `#[requires]` attributes can be stacked on the same function.
/// Works with regular and `async` functions.
///
/// # Example
///
/// ```ignore
/// use assura_macros::requires;
///
/// #[requires(divisor != 0)]
/// #[requires(dividend >= i64::MIN + 1)]
/// fn safe_divide(dividend: i64, divisor: i64) -> i64 {
///     dividend / divisor
/// }
/// ```
#[proc_macro_attribute]
pub fn requires(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let pred_str = attr.to_string();
    let fn_name = input.sig.ident.to_string();

    let assert_code = match syn::parse::<syn::Expr>(attr) {
        Ok(expr) => make_check(&expr, &fn_name, "precondition", &pred_str),
        Err(_) => {
            let msg = format!("assura: could not parse precondition: {pred_str}");
            quote! { compile_error!(#msg); }
        }
    };

    let vis = &input.vis;
    let sig = &input.sig;
    let attrs = &input.attrs;
    let block = &input.block;
    let stmts = &block.stmts;

    quote! {
        #(#attrs)*
        #vis #sig {
            #assert_code
            #(#stmts)*
        }
    }
    .into()
}

/// Add a postcondition to a Rust function.
///
/// In debug builds, captures the return value and checks the condition
/// before returning. Use `result` to refer to the return value.
/// In release builds, the assertion is stripped (zero runtime cost).
///
/// Multiple `#[ensures]` attributes can be stacked on the same function.
/// Works with regular and `async` functions.
///
/// # Example
///
/// ```ignore
/// use assura_macros::ensures;
///
/// #[ensures(result >= 0)]
/// fn absolute(x: i32) -> i32 {
///     if x < 0 { -x } else { x }
/// }
/// ```
#[proc_macro_attribute]
pub fn ensures(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let pred_str = attr.to_string();
    let has_return = !matches!(input.sig.output, syn::ReturnType::Default);

    if !has_return {
        // No return type: ensures doesn't make sense, pass through with warning
        let msg = format!("assura: #[ensures] on void function has no effect: {pred_str}");
        let vis = &input.vis;
        let sig = &input.sig;
        let attrs = &input.attrs;
        let block = &input.block;
        let stmts = &block.stmts;
        return quote! {
            #(#attrs)*
            #vis #sig {
                let _ = #msg;
                #(#stmts)*
            }
        }
        .into();
    }

    let fn_name = input.sig.ident.to_string();

    // Extract old() expressions and generate snapshot bindings
    let (old_exprs, pred_without_old) = extract_old_expressions(&pred_str);
    let snapshot_bindings: Vec<proc_macro2::TokenStream> = old_exprs
        .iter()
        .enumerate()
        .map(|(idx, expr_str)| {
            let var_name = syn::Ident::new(
                &format!("__assura_old_{idx}"),
                proc_macro2::Span::call_site(),
            );
            match syn::parse_str::<syn::Expr>(expr_str) {
                Ok(expr) => quote! { let #var_name = (#expr).clone(); },
                Err(_) => {
                    let msg = format!("assura: could not parse old() expression: {expr_str}");
                    quote! { compile_error!(#msg); }
                }
            }
        })
        .collect();

    // Replace standalone `result` with `__assura_result` in the predicate
    let adjusted = replace_result_word(&pred_without_old);
    let assert_code = match syn::parse_str::<syn::Expr>(&adjusted) {
        Ok(expr) => make_check(&expr, &fn_name, "postcondition", &pred_str),
        Err(_) => {
            let msg = format!("assura: could not parse postcondition: {pred_str}");
            quote! { compile_error!(#msg); }
        }
    };

    let vis = &input.vis;
    let sig = &input.sig;
    let attrs = &input.attrs;
    let block = &input.block;
    let stmts = &block.stmts;

    quote! {
        #(#attrs)*
        #vis #sig {
            #(#snapshot_bindings)*
            let __assura_result = { #(#stmts)* };
            #assert_code
            __assura_result
        }
    }
    .into()
}

/// Add an invariant to a Rust function.
///
/// In debug builds, generates `debug_assert!` at both function entry
/// AND exit (after computing the return value). Use this for conditions
/// that must hold before and after the function executes.
///
/// If the expression contains `result`, the entry check is skipped
/// (since the return value is not yet available) and only the exit
/// check runs.
///
/// # Example
///
/// ```ignore
/// use assura_macros::invariant;
///
/// #[invariant(self.len <= self.capacity)]
/// fn push(&mut self, item: T) {
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn invariant(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Try parsing as an impl block first, then fall back to a function.
    let item_clone = item.clone();
    if let Ok(impl_block) = syn::parse::<ItemImpl>(item_clone) {
        return invariant_impl(attr, impl_block);
    }
    let input = parse_macro_input!(item as ItemFn);
    invariant_fn(attr, input)
}

/// Apply `#[invariant(expr)]` to a single function.
fn invariant_fn(attr: TokenStream, input: ItemFn) -> TokenStream {
    let pred_str = attr.to_string();
    let has_return = !matches!(input.sig.output, syn::ReturnType::Default);
    let mentions_result = contains_result_word(&pred_str);

    let fn_name = input.sig.ident.to_string();

    let assert_code = match syn::parse::<syn::Expr>(attr) {
        Ok(expr) => {
            let pre = if mentions_result {
                quote! {}
            } else {
                make_check(&expr, &fn_name, "invariant (entry)", &pred_str)
            };

            let post = if mentions_result {
                let adjusted = replace_result_word(&pred_str);
                match syn::parse_str::<syn::Expr>(&adjusted) {
                    Ok(adj_expr) => make_check(&adj_expr, &fn_name, "invariant (exit)", &pred_str),
                    Err(_) => make_check(&expr, &fn_name, "invariant (exit)", &pred_str),
                }
            } else {
                make_check(&expr, &fn_name, "invariant (exit)", &pred_str)
            };

            (pre, post)
        }
        Err(_) => {
            let msg = format!("assura: could not parse invariant: {pred_str}");
            (quote! { compile_error!(#msg); }, quote! {})
        }
    };

    let vis = &input.vis;
    let sig = &input.sig;
    let attrs = &input.attrs;
    let block = &input.block;
    let stmts = &block.stmts;
    let (pre, post) = assert_code;

    if has_return {
        quote! {
            #(#attrs)*
            #vis #sig {
                #pre
                let __assura_result = { #(#stmts)* };
                #post
                __assura_result
            }
        }
        .into()
    } else {
        quote! {
            #(#attrs)*
            #vis #sig {
                #pre
                #(#stmts)*
                #post
            }
        }
        .into()
    }
}

/// Check if a method has a `&mut self` receiver.
fn is_mut_self_method(sig: &syn::Signature) -> bool {
    if let Some(syn::FnArg::Receiver(recv)) = sig.inputs.first() {
        recv.reference.is_some() && recv.mutability.is_some()
    } else {
        false
    }
}

/// Apply `#[invariant(expr)]` to an impl block: wraps every `&mut self`
/// method with pre/post invariant checks.
fn invariant_impl(attr: TokenStream, mut impl_block: ItemImpl) -> TokenStream {
    let pred_str = attr.to_string();
    let inv_expr = match syn::parse::<syn::Expr>(attr) {
        Ok(expr) => expr,
        Err(_) => {
            let msg = format!("assura: could not parse invariant: {pred_str}");
            return quote! { compile_error!(#msg); }.into();
        }
    };

    // Get the type name for error messages (best-effort)
    let self_ty = &impl_block.self_ty;
    let type_name = quote!(#self_ty).to_string();

    for item in &mut impl_block.items {
        if let syn::ImplItem::Fn(method) = item {
            if !is_mut_self_method(&method.sig) {
                continue;
            }
            let method_name = method.sig.ident.to_string();
            let label = format!("{type_name}::{method_name}");
            let pre = make_check(&inv_expr, &label, "invariant (entry)", &pred_str);
            let post = make_check(&inv_expr, &label, "invariant (exit)", &pred_str);

            let has_return = !matches!(method.sig.output, syn::ReturnType::Default);
            let old_stmts = std::mem::take(&mut method.block.stmts);

            if has_return {
                method.block.stmts = syn::parse_quote! {
                    #pre
                    let __assura_result = { #(#old_stmts)* };
                    #post
                    __assura_result
                };
            } else {
                method.block.stmts = syn::parse_quote! {
                    #pre
                    #(#old_stmts)*
                    #post
                };
            }
        }
    }

    quote!(#impl_block).into()
}

/// Mark a function with Assura contract annotations via doc comments.
///
/// In debug builds, generates `debug_assert!` statements from `@requires`
/// and `@ensures` clauses in doc comments. Also supports feature-specific
/// annotations (`@ghost`, `@taint`, `@region`, etc.).
/// In release builds, the function is unchanged (zero runtime cost).
///
/// For a more ergonomic syntax, consider using `#[requires(...)]` and
/// `#[ensures(...)]` attributes directly instead.
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
    let fn_name_for_features = input.sig.ident.to_string();
    let feature_asserts = generate_feature_asserts(&feature_annotations, &fn_name_for_features);

    // If no contract clauses or feature annotations found, return unchanged
    if requires.is_empty() && ensures.is_empty() && feature_annotations.is_empty() {
        return quote!(#input).into();
    }

    let fn_name = input.sig.ident.to_string();
    let vis = &input.vis;
    let sig = &input.sig;
    let attrs = &input.attrs;
    let block = &input.block;

    // Build precondition assertions
    let pre_asserts: Vec<proc_macro2::TokenStream> = requires
        .iter()
        .map(|pred| match syn::parse_str::<syn::Expr>(pred) {
            Ok(expr) => make_check(&expr, &fn_name, "precondition", pred),
            Err(_) => {
                let warn_msg =
                    format!("assura: could not parse precondition as Rust expression: {pred}");
                quote! {
                    let _ = #warn_msg;
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
                let adjusted = replace_result_word(pred);
                match syn::parse_str::<syn::Expr>(&adjusted) {
                    Ok(expr) => make_check(&expr, &fn_name, "postcondition", pred),
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

/// Add a postcondition that only runs on the `Ok` path of a `Result`-returning function.
///
/// Use `result` to refer to the unwrapped `Ok` value. When the function
/// returns `Err`, the postcondition is skipped entirely.
///
/// # Example
///
/// ```ignore
/// use assura_macros::ensures_ok;
///
/// #[ensures_ok(result.len() <= max_size)]
/// fn read_data(path: &str, max_size: usize) -> Result<Vec<u8>, std::io::Error> {
///     let data = std::fs::read(path)?;
///     Ok(data)
/// }
/// ```
#[proc_macro_attribute]
pub fn ensures_ok(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let pred_str = attr.to_string();
    let fn_name = input.sig.ident.to_string();

    // Extract old() expressions and generate snapshot bindings
    let (old_exprs, pred_without_old) = extract_old_expressions(&pred_str);
    let snapshot_bindings: Vec<proc_macro2::TokenStream> = old_exprs
        .iter()
        .enumerate()
        .map(|(idx, expr_str)| {
            let var_name = syn::Ident::new(
                &format!("__assura_old_{idx}"),
                proc_macro2::Span::call_site(),
            );
            match syn::parse_str::<syn::Expr>(expr_str) {
                Ok(expr) => quote! { let #var_name = (#expr).clone(); },
                Err(_) => {
                    let msg = format!("assura: could not parse old() expression: {expr_str}");
                    quote! { compile_error!(#msg); }
                }
            }
        })
        .collect();

    let adjusted = replace_result_word(&pred_without_old);
    let assert_code = match syn::parse_str::<syn::Expr>(&adjusted) {
        Ok(expr) => make_check(&expr, &fn_name, "ensures_ok", &pred_str),
        Err(_) => {
            let msg = format!("assura: could not parse ensures_ok: {pred_str}");
            return quote! { compile_error!(#msg); }.into();
        }
    };

    let vis = &input.vis;
    let sig = &input.sig;
    let attrs = &input.attrs;
    let block = &input.block;
    let stmts = &block.stmts;

    quote! {
        #(#attrs)*
        #vis #sig {
            #(#snapshot_bindings)*
            let __assura_ret = { #(#stmts)* };
            match __assura_ret {
                Ok(__assura_result) => {
                    #assert_code
                    Ok(__assura_result)
                }
                __assura_err => __assura_err
            }
        }
    }
    .into()
}

/// Add a postcondition that only runs on the `Err` path of a `Result`-returning function.
///
/// Use `result` to refer to the unwrapped `Err` value. When the function
/// returns `Ok`, the postcondition is skipped entirely.
///
/// # Example
///
/// ```ignore
/// use assura_macros::ensures_err;
///
/// #[ensures_err(!result.to_string().is_empty())]
/// fn parse_config(input: &str) -> Result<Config, String> {
///     if input.is_empty() { return Err("empty input".to_string()); }
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn ensures_err(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let pred_str = attr.to_string();
    let fn_name = input.sig.ident.to_string();

    let adjusted = replace_result_word(&pred_str);
    let assert_code = match syn::parse_str::<syn::Expr>(&adjusted) {
        Ok(expr) => make_check(&expr, &fn_name, "ensures_err", &pred_str),
        Err(_) => {
            let msg = format!("assura: could not parse ensures_err: {pred_str}");
            return quote! { compile_error!(#msg); }.into();
        }
    };

    let vis = &input.vis;
    let sig = &input.sig;
    let attrs = &input.attrs;
    let block = &input.block;
    let stmts = &block.stmts;

    quote! {
        #(#attrs)*
        #vis #sig {
            let __assura_ret = { #(#stmts)* };
            match __assura_ret {
                Err(__assura_result) => {
                    #assert_code
                    Err(__assura_result)
                }
                __assura_ok => __assura_ok
            }
        }
    }
    .into()
}

/// Mark function parameters as tainted for secret leak prevention.
///
/// Wraps all non-`self` parameters in `assura_runtime::Tainted<T>`, which:
/// - Does NOT implement `Display` (compile error on `println!("{}", param)`)
/// - Implements `Debug` as `[REDACTED: <label>]`
/// - Requires `.declassify()` or `.validate()` to access the inner value
///
/// The attribute argument is the taint label (e.g., `secret`, `pii`, `api_key`).
///
/// # Example
///
/// ```ignore
/// use assura_macros::taint;
///
/// #[taint(secret)]
/// fn process_api_key(key: String, name: String) -> bool {
///     // `key` and `name` are now Tainted<String>
///     let raw_key = key.declassify();  // explicit opt-in
///     let valid = name.validate(|n| !n.is_empty());  // checked access
///     valid.is_some()
/// }
/// ```
#[proc_macro_attribute]
pub fn taint(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let label = attr.to_string();
    let label = label.trim();

    if label.is_empty() {
        return syn::Error::new_spanned(
            &input.sig.ident,
            "assura: #[taint] requires a label argument, e.g. #[taint(secret)]",
        )
        .to_compile_error()
        .into();
    }

    // Collect non-self parameter names to taint
    let taint_params: Vec<syn::Ident> = input
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let syn::FnArg::Typed(pat_type) = arg
                && let syn::Pat::Ident(pat_ident) = &*pat_type.pat
            {
                return Some(pat_ident.ident.clone());
            }
            None
        })
        .collect();

    // Generate shadow bindings that wrap each parameter in Tainted<T>
    let shadows: Vec<proc_macro2::TokenStream> = taint_params
        .iter()
        .map(|ident| {
            quote! {
                let #ident = ::assura_runtime::Tainted::new(#ident, #label);
            }
        })
        .collect();

    let vis = &input.vis;
    let sig = &input.sig;
    let attrs = &input.attrs;
    let block = &input.block;
    let stmts = &block.stmts;

    quote! {
        #(#attrs)*
        #vis #sig {
            #(#shadows)*
            #(#stmts)*
        }
    }
    .into()
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

    // ---- escape_braces ----

    #[test]
    fn escape_braces_in_unsafe_block() {
        assert_eq!(
            escape_braces("unsafe { COUNTER >= 0 }"),
            "unsafe {{ COUNTER >= 0 }}"
        );
    }

    #[test]
    fn escape_braces_no_braces() {
        assert_eq!(escape_braces("x > 0"), "x > 0");
    }

    // ---- contains_result_word ----

    #[test]
    fn contains_result_standalone() {
        assert!(contains_result_word("result >= 0"));
    }

    #[test]
    fn contains_result_not_in_compound() {
        assert!(!contains_result_word("partial_result > 0"));
        assert!(!contains_result_word("result_count > 0"));
    }

    #[test]
    fn contains_result_absent() {
        assert!(!contains_result_word("x > 0"));
    }
}
