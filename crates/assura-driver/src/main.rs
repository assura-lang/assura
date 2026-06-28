//! Custom rustc driver for compiler-level contract verification.
//!
//! This binary hooks into the Rust compiler via `rustc_driver::Callbacks` to
//! extract fully resolved type information, MIR control flow, and call graphs
//! for functions annotated with Assura contract attributes (`#[requires]`,
//! `#[ensures]`, `#[invariant]`).
//!
//! # Requirements
//!
//! - Nightly Rust toolchain
//! - `rustc-dev` component: `rustup component add rustc-dev --toolchain nightly`
//!
//! # Usage
//!
//! ```bash
//! # Build
//! cargo +nightly build -p assura-driver
//!
//! # Run on a crate (acts as a rustc wrapper)
//! RUSTC_WRAPPER=target/debug/assura-driver cargo +nightly check
//!
//! # Or directly
//! target/debug/assura-driver src/main.rs --edition 2021
//! ```
//!
//! The driver prints a JSON `CompilerAnalysis` to stdout with all annotated
//! functions, their resolved types, MIR summaries, and call graph edges.

#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

use std::process;

use rustc_driver::{Callbacks, Compilation};
use rustc_hir::def_id::{DefId, LocalDefId};
use rustc_hir::{ItemKind, Node};
use rustc_middle::mir::TerminatorKind;
use rustc_middle::ty::TyCtxt;
use rustc_span::Symbol;

use serde::Serialize;

// ---------------------------------------------------------------------------
// Output types (serialized to JSON for the LLM/Z3 pipeline)
// ---------------------------------------------------------------------------

/// Top-level output from the compiler driver.
#[derive(Debug, Serialize)]
pub struct CompilerAnalysis {
    /// Functions with contract annotations and their compiler-resolved info.
    pub annotated_functions: Vec<AnnotatedFunction>,
    /// Total number of functions scanned (with or without contracts).
    pub total_functions: usize,
}

/// A function with Assura contract attributes, enriched with compiler data.
#[derive(Debug, Serialize)]
pub struct AnnotatedFunction {
    /// Fully qualified name (e.g., `crate::security::PathGuard::check_path`).
    pub name: String,
    /// The module path containing this function.
    pub module_path: String,
    /// Contract clauses from attributes.
    pub contracts: Vec<ContractClause>,
    /// Parameters with fully resolved types.
    pub params: Vec<TypedParam>,
    /// Fully resolved return type.
    pub return_type: String,
    /// MIR summary: basic blocks, control flow.
    pub mir_summary: MirSummary,
    /// Resolved call sites in the function body.
    pub callees: Vec<CallSite>,
}

/// A contract clause extracted from a `#[requires(...)]` or `#[ensures(...)]` attribute.
#[derive(Debug, Serialize)]
pub struct ContractClause {
    pub kind: String,
    pub expression: String,
}

/// A function parameter with its resolved type.
#[derive(Debug, Serialize)]
pub struct TypedParam {
    pub name: String,
    pub ty: String,
}

/// Summary of MIR control flow for a function.
#[derive(Debug, Serialize)]
pub struct MirSummary {
    pub basic_block_count: usize,
    pub return_points: usize,
    pub has_loops: bool,
    pub has_panicking_paths: bool,
    pub has_unsafe_blocks: bool,
}

/// A resolved call site within a function body.
#[derive(Debug, Serialize)]
pub struct CallSite {
    /// Name of the called function.
    pub callee_name: String,
    /// Fully qualified path of the callee.
    pub callee_path: String,
    /// Whether this was resolved through trait dispatch.
    pub is_trait_dispatch: bool,
    /// Any contracts on the callee function.
    pub callee_contracts: Vec<ContractClause>,
}

// ---------------------------------------------------------------------------
// Driver implementation
// ---------------------------------------------------------------------------

struct AssuraDriver {
    /// Collect output here; printed to stdout after analysis.
    output: Option<CompilerAnalysis>,
}

impl Callbacks for AssuraDriver {
    fn after_analysis(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        tcx: TyCtxt<'_>,
    ) -> Compilation {
        let mut annotated = Vec::new();
        let mut total_functions = 0usize;

        // Walk all items in the crate
        for item_id in tcx.hir_crate_items(()).free_items() {
            let item = tcx.hir_node(item_id.into());
            let Node::Item(item) = item else {
                continue;
            };

            match &item.kind {
                ItemKind::Fn { .. } => {
                    total_functions += 1;
                    let local_def_id = item.owner_id.def_id;
                    let def_id = local_def_id.to_def_id();

                    let contracts = extract_contracts(tcx, def_id);
                    if contracts.is_empty() {
                        continue;
                    }

                    let name = tcx.def_path_str(def_id);
                    let module_path = tcx
                        .parent_module(item.hir_id())
                        .to_def_id()
                        .as_local()
                        .map(|id| tcx.def_path_str(id.to_def_id()))
                        .unwrap_or_default();

                    let params = extract_params(tcx, local_def_id);
                    let return_type = extract_return_type(tcx, local_def_id);
                    let mir_summary = extract_mir_summary(tcx, local_def_id);
                    let callees = extract_callees(tcx, local_def_id);

                    annotated.push(AnnotatedFunction {
                        name,
                        module_path,
                        contracts,
                        params,
                        return_type,
                        mir_summary,
                        callees,
                    });
                }
                ItemKind::Impl(impl_block) => {
                    // Check methods inside impl blocks
                    for impl_item_ref in impl_block.items {
                        let impl_item = tcx.hir_impl_item(impl_item_ref.id);
                        if let rustc_hir::ImplItemKind::Fn(_, _) = &impl_item.kind {
                            total_functions += 1;
                            let local_def_id = impl_item.owner_id.def_id;
                            let def_id = local_def_id.to_def_id();

                            let contracts = extract_contracts(tcx, def_id);
                            if contracts.is_empty() {
                                continue;
                            }

                            let name = tcx.def_path_str(def_id);
                            let module_path = tcx.def_path_str(
                                tcx.parent(def_id),
                            );
                            let params = extract_params(tcx, local_def_id);
                            let return_type = extract_return_type(tcx, local_def_id);
                            let mir_summary = extract_mir_summary(tcx, local_def_id);
                            let callees = extract_callees(tcx, local_def_id);

                            annotated.push(AnnotatedFunction {
                                name,
                                module_path,
                                contracts,
                                params,
                                return_type,
                                mir_summary,
                                callees,
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        self.output = Some(CompilerAnalysis {
            annotated_functions: annotated,
            total_functions,
        });

        Compilation::Continue
    }
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

/// Extract Assura contract attributes from a definition.
fn extract_contracts(tcx: TyCtxt<'_>, def_id: DefId) -> Vec<ContractClause> {
    let mut contracts = Vec::new();

    for attr in tcx.get_attrs_unchecked(def_id) {
        // Look for #[requires(...)], #[ensures(...)], #[invariant(...)]
        let path = attr.path();
        let segments: Vec<_> = path.segments.iter().map(|s| s.ident.name).collect();

        let kind = match segments.last() {
            Some(name) if *name == Symbol::intern("requires") => "requires",
            Some(name) if *name == Symbol::intern("ensures") => "ensures",
            Some(name) if *name == Symbol::intern("invariant") => "invariant",
            Some(name) if *name == Symbol::intern("decreases") => "decreases",
            Some(name) if *name == Symbol::intern("ensures_ok") => "ensures_ok",
            Some(name) if *name == Symbol::intern("ensures_err") => "ensures_err",
            _ => continue,
        };

        // Extract the expression from the attribute arguments
        let expression = attr
            .value_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                // For #[requires(expr)], extract from token stream
                if let Some(args) = attr.meta_item_list() {
                    args.iter()
                        .map(|a| a.name_or_empty().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                } else {
                    // Fall back to debug printing the attribute
                    format!("{attr:?}")
                }
            });

        contracts.push(ContractClause {
            kind: kind.to_string(),
            expression,
        });
    }

    contracts
}

/// Extract function parameters with resolved types.
fn extract_params(tcx: TyCtxt<'_>, def_id: LocalDefId) -> Vec<TypedParam> {
    let fn_sig = tcx.fn_sig(def_id).instantiate_identity();
    let fn_sig = tcx.liberate_late_bound_regions(def_id.to_def_id(), fn_sig);

    // Get parameter names from HIR
    let hir_id = tcx.local_def_id_to_hir_id(def_id);
    let body_id = match tcx.hir_node(hir_id) {
        Node::Item(item) => {
            if let ItemKind::Fn { body, .. } = &item.kind {
                *body
            } else {
                return vec![];
            }
        }
        Node::ImplItem(item) => {
            if let rustc_hir::ImplItemKind::Fn(_, body_id) = &item.kind {
                *body_id
            } else {
                return vec![];
            }
        }
        _ => return vec![],
    };

    let body = tcx.hir_body(body_id);
    let param_names: Vec<String> = body
        .params
        .iter()
        .map(|p| match &p.pat.kind {
            rustc_hir::PatKind::Binding(_, _, ident, _) => ident.name.to_string(),
            _ => "_".to_string(),
        })
        .collect();

    fn_sig
        .inputs()
        .iter()
        .enumerate()
        .map(|(i, ty)| TypedParam {
            name: param_names
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("arg{i}")),
            ty: format!("{ty}"),
        })
        .collect()
}

/// Extract the return type as a string.
fn extract_return_type(tcx: TyCtxt<'_>, def_id: LocalDefId) -> String {
    let fn_sig = tcx.fn_sig(def_id).instantiate_identity();
    let fn_sig = tcx.liberate_late_bound_regions(def_id.to_def_id(), fn_sig);
    format!("{}", fn_sig.output())
}

/// Extract a MIR summary for a function.
fn extract_mir_summary(tcx: TyCtxt<'_>, def_id: LocalDefId) -> MirSummary {
    // Use mir_built or optimized_mir depending on availability
    let mir = tcx.optimized_mir(def_id.to_def_id());

    let basic_block_count = mir.basic_blocks.len();
    let mut return_points = 0;
    let mut has_loops = false;
    let mut has_panicking_paths = false;
    let mut has_unsafe_blocks = false;

    for (bb_idx, bb_data) in mir.basic_blocks.iter_enumerated() {
        if let Some(terminator) = &bb_data.terminator {
            match &terminator.kind {
                TerminatorKind::Return => return_points += 1,
                TerminatorKind::UnwindResume | TerminatorKind::Unreachable => {
                    has_panicking_paths = true;
                }
                TerminatorKind::Goto { target } => {
                    // Back edge = loop
                    if *target <= bb_idx {
                        has_loops = true;
                    }
                }
                _ => {}
            }
        }
    }

    // Check for unsafe blocks in the source
    // (MIR does not directly expose this, but we can check the HIR)
    let hir_id = tcx.local_def_id_to_hir_id(def_id);
    if let Node::Item(item) = tcx.hir_node(hir_id) {
        if let ItemKind::Fn { .. } = &item.kind {
            has_unsafe_blocks = tcx.fn_sig(def_id).skip_binder().safety()
                == rustc_hir::Safety::Unsafe;
        }
    }

    MirSummary {
        basic_block_count,
        return_points,
        has_loops,
        has_panicking_paths,
        has_unsafe_blocks,
    }
}

/// Extract call sites with resolved callees from MIR.
fn extract_callees(tcx: TyCtxt<'_>, def_id: LocalDefId) -> Vec<CallSite> {
    let mir = tcx.optimized_mir(def_id.to_def_id());
    let mut callees = Vec::new();

    for bb_data in mir.basic_blocks.iter() {
        if let Some(terminator) = &bb_data.terminator {
            if let TerminatorKind::Call { func, .. } = &terminator.kind {
                let ty = func.ty(&mir.local_decls, tcx);

                if let rustc_middle::ty::TyKind::FnDef(callee_def_id, _substs) = ty.kind() {
                    let callee_name = tcx.item_name(*callee_def_id).to_string();
                    let callee_path = tcx.def_path_str(*callee_def_id);

                    // Check if this is a trait method (trait dispatch)
                    let is_trait_dispatch = tcx
                        .trait_of_item(*callee_def_id)
                        .is_some();

                    // Look up contracts on the callee
                    let callee_contracts = extract_contracts(tcx, *callee_def_id);

                    callees.push(CallSite {
                        callee_name,
                        callee_path,
                        is_trait_dispatch,
                        callee_contracts,
                    });
                }
            }
        }
    }

    // Deduplicate by callee_path
    callees.sort_by(|a, b| a.callee_path.cmp(&b.callee_path));
    callees.dedup_by(|a, b| a.callee_path == b.callee_path);

    callees
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    // When used as RUSTC_WRAPPER, argv[1] is the path to rustc.
    // When used directly, argv[1] is the source file.
    let mut args: Vec<String> = std::env::args().collect();

    // If first arg looks like a rustc binary, remove it (RUSTC_WRAPPER mode)
    if args.len() > 1 && (args[1].ends_with("rustc") || args[1].contains("rustc")) {
        args.remove(1);
    }

    // Add --sysroot if not already present
    if !args.iter().any(|a| a == "--sysroot") {
        let sysroot = std::process::Command::new("rustc")
            .arg("--print=sysroot")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        if !sysroot.is_empty() {
            args.push("--sysroot".to_string());
            args.push(sysroot);
        }
    }

    let mut driver = AssuraDriver { output: None };

    let exit_code = rustc_driver::catch_with_exit_code(|| {
        let mut compiler = RunCompiler::new(&args, &mut driver);
        compiler.run()
    });

    // Print analysis output as JSON
    if let Some(analysis) = &driver.output {
        if !analysis.annotated_functions.is_empty() {
            match serde_json::to_string_pretty(analysis) {
                Ok(json) => println!("{json}"),
                Err(e) => eprintln!("assura-driver: failed to serialize output: {e}"),
            }
        } else {
            eprintln!(
                "assura-driver: scanned {} functions, none had contract annotations",
                analysis.total_functions
            );
        }
    }

    process::exit(exit_code);
}