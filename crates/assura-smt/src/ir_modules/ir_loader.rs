//! Load Implementation IR sidecars for havoc+assume verification (#273).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use assura_ast::{Decl, ServiceItem};

use crate::VerifyFileExtras;
use crate::ir::{IrFunction, IrInstr, IrModule, parse_ir_module};
use crate::ir_encode::block_map_from_module;

/// IR sidecars loaded for a source file, with a borrowed view for verification APIs.
pub struct LoadedVerifyExtras {
    pub(crate) ir_map: HashMap<String, IrFunction>,
    pub(crate) block_map: HashMap<String, HashMap<usize, Vec<IrInstr>>>,
    /// Contracts filled from ensures heuristics (no co-located `.ir` on disk).
    pub(crate) heuristic_names: Vec<String>,
}

impl LoadedVerifyExtras {
    /// Load `{ContractName}.ir` sidecars for all verification jobs in `typed`.
    pub fn load(source_file: &Path, typed: &assura_types::TypedFile) -> Self {
        let (ir_map, block_map) = load_ir_sidecars_for_typed(source_file, typed);
        Self {
            ir_map,
            block_map,
            heuristic_names: Vec::new(),
        }
    }

    /// Load co-located sidecars, then synthesize **analyzable** heuristic IR
    /// in memory for any remaining jobs (no disk write).
    ///
    /// Pure stubs (`Stub IR` fallback for unanalyzable ensures) are **not**
    /// injected, so `ensures { result > 0 }` still reports Unknown instead of
    /// a false identity proof. Analyzable shapes (`result == x`, arith, call
    /// chains with same-file callees, match/if, …) verify without requiring
    /// the user to run `--write-ir` first.
    pub fn load_or_synthesize(source_file: &Path, typed: &assura_types::TypedFile) -> Self {
        let mut loaded = Self::load(source_file, typed);
        loaded.fill_missing_with_heuristics(typed);
        loaded
    }

    /// Fill missing co-located IR from ensures heuristics (in memory only).
    pub fn fill_missing_with_heuristics(&mut self, typed: &assura_types::TypedFile) {
        let heuristics = stub_ir_sidecars_for_typed(typed);
        for (name, text) in heuristics {
            if self.ir_map.contains_key(&name) {
                continue;
            }
            // Unanalyzable ensures produce a labeled stub; do not pretend it proves result.
            if is_stub_ir_text(&text) {
                continue;
            }
            let Ok(module) = parse_ir_module(&text) else {
                continue;
            };
            let Some(func) = module.functions.first() else {
                continue;
            };
            self.ir_map.insert(name.clone(), func.clone());
            let blocks = block_map_from_module(&module);
            if !blocks.is_empty() {
                self.block_map.insert(name.clone(), blocks);
            }
            self.heuristic_names.push(name);
        }
        self.heuristic_names.sort();
    }

    /// Names that used in-memory heuristic IR (not co-located files).
    pub fn heuristic_names(&self) -> &[String] {
        &self.heuristic_names
    }

    /// Build extras from in-memory IR text, mapping the first function to
    /// `contract_name`. Used by the AI verification loop (12.01) where IR
    /// is submitted inline rather than loaded from sidecar files.
    pub fn from_ir_text(ir_source: &str, contract_name: &str) -> Result<Self, Vec<String>> {
        let module = parse_ir_module(ir_source)?;
        let mut ir_map = HashMap::new();
        let mut block_map_out = HashMap::new();
        if let Some(func) = module.functions.first() {
            ir_map.insert(contract_name.to_string(), func.clone());
            let blocks = block_map_from_module(&module);
            if !blocks.is_empty() {
                block_map_out.insert(contract_name.to_string(), blocks);
            }
        }
        Ok(Self {
            ir_map,
            block_map: block_map_out,
            heuristic_names: Vec::new(),
        })
    }

    /// Whether any sidecar IR bodies were discovered.
    pub fn is_empty(&self) -> bool {
        self.ir_map.is_empty()
    }

    /// Names of contracts/functions that have a loaded IR sidecar (sorted).
    pub fn loaded_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.ir_map.keys().cloned().collect();
        names.sort();
        names
    }

    /// `Some(VerifyFileExtras)` when sidecars exist; `None` otherwise.
    pub fn extras(&self) -> Option<VerifyFileExtras<'_>> {
        (!self.ir_map.is_empty()).then_some(VerifyFileExtras {
            ir_bodies: Some(&self.ir_map),
            ir_blocks: Some(&self.block_map),
            type_env: None,
            ir_loading_attempted: true,
        })
    }
}

/// Directories to search for `{contract_name}.ir` sidecars near a source file.
pub fn ir_search_dirs_for_source(source_file: &Path) -> Vec<PathBuf> {
    let parent = source_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    vec![parent.clone(), parent.join("generated")]
}

/// Contract names from a typed file (same set as `collect_verification_jobs`).
pub fn collect_verification_job_names(typed: &assura_types::TypedFile) -> Vec<String> {
    let mut names = Vec::new();
    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Contract(c) => names.push(c.name.clone()),
            Decl::FnDef(f) => names.push(f.name.clone()),
            Decl::Extern(e) => names.push(e.name.clone()),
            Decl::Service(s) => {
                for item in &s.items {
                    match item {
                        ServiceItem::Operation { name, .. } => {
                            names.push(format!("{}.{}", s.name, name));
                        }
                        ServiceItem::Query { name, .. } => {
                            names.push(format!("{}.{}", s.name, name));
                        }
                        ServiceItem::Invariant(_) => {
                            names.push(crate::verify_labels::invariant_desc(&s.name));
                        }
                        _ => {}
                    }
                }
            }
            Decl::Block { name, .. } => names.push(name.clone()),
            Decl::Bind(b) => names.push(b.name.clone()),
            _ => {}
        }
    }
    names
}

/// Parsed IR sidecar: entry function plus optional block bodies from sibling `fn #N`.
pub(crate) struct IrSidecar {
    func: IrFunction,
    blocks: HashMap<usize, Vec<IrInstr>>,
}

/// Load IR bodies for the given contract names from sidecar `.ir` files.
pub fn load_ir_bodies_for_contracts(
    search_dirs: &[&Path],
    contract_names: &[String],
) -> HashMap<String, IrFunction> {
    let mut out = HashMap::new();
    for name in contract_names {
        if let Some(sidecar) = resolve_ir_sidecar(search_dirs, name) {
            out.insert(name.clone(), sidecar.func);
        }
    }
    out
}

/// Functions and per-contract `fn #N` block maps loaded from sidecars.
pub type LoadedIrSidecars = (
    HashMap<String, IrFunction>,
    HashMap<String, HashMap<usize, Vec<IrInstr>>>,
);

/// Load IR sidecars (functions + block maps) for all verification jobs in a typed file.
pub fn load_ir_sidecars_for_typed(
    source_file: &Path,
    typed: &assura_types::TypedFile,
) -> LoadedIrSidecars {
    let dirs = ir_search_dirs_for_source(source_file);
    let dir_refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
    let names = collect_verification_job_names(typed);
    let mut ir_map = HashMap::new();
    let mut block_map = HashMap::new();
    for name in &names {
        if let Some(sidecar) = resolve_ir_sidecar(&dir_refs, name) {
            ir_map.insert(name.clone(), sidecar.func);
            if !sidecar.blocks.is_empty() {
                block_map.insert(name.clone(), sidecar.blocks);
            }
        }
    }
    (ir_map, block_map)
}

/// Load IR sidecars for all verification jobs in a typed file (functions only).
pub fn load_ir_bodies_for_typed(
    source_file: &Path,
    typed: &assura_types::TypedFile,
) -> HashMap<String, IrFunction> {
    load_ir_sidecars_for_typed(source_file, typed).0
}

pub(crate) fn resolve_ir_sidecar(search_dirs: &[&Path], contract_name: &str) -> Option<IrSidecar> {
    let file_name = format!("{contract_name}.ir");
    for dir in search_dirs {
        let path = dir.join(&file_name);
        if let Some(sidecar) = load_ir_file(&path) {
            return Some(sidecar);
        }
    }
    None
}

/// Emit stub `.ir` sidecar text for every verification job in a typed file.
///
/// Builds a same-file callee map first so call-shaped ensures can synthesize
/// non-identity sibling bodies from peer contracts/fns (#863).
pub fn stub_ir_sidecars_for_typed(typed: &assura_types::TypedFile) -> HashMap<String, String> {
    use crate::ir_generate::{CalleeSpec, generate_ir_sidecar_text_with_callees};

    let jobs = crate::entry::collect_verification_jobs(typed);
    let mut callees: HashMap<String, CalleeSpec> = HashMap::new();
    for (name, clauses, params, return_ty) in &jobs {
        let ret = if return_ty.is_empty() {
            "Unit".into()
        } else {
            return_ty.join(" ")
        };
        callees.insert(
            name.clone(),
            CalleeSpec {
                param_names: params.iter().map(|p| p.name.clone()).collect(),
                return_ty: ret,
                clauses: clauses.clone(),
            },
        );
    }

    let mut out = HashMap::new();
    for (name, clauses, params, return_ty) in &jobs {
        let param_tys: Vec<(usize, String)> = params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    i,
                    p.ty.as_ref()
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "Int".into()),
                )
            })
            .collect();
        let ret = if return_ty.is_empty() {
            "Unit".into()
        } else {
            return_ty.join(" ")
        };
        let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        out.insert(
            name.clone(),
            generate_ir_sidecar_text_with_callees(
                name,
                &param_tys,
                &param_names,
                &ret,
                clauses,
                &callees,
            ),
        );
    }
    out
}

/// True when text is a placeholder stub (unanalyzable ensures), not a real body.
///
/// These must not be treated as co-located implementation IR for verify/codegen:
/// they use identity `load $0` and would prove the wrong thing or poison `--write-ir`.
pub fn is_stub_ir_text(source: &str) -> bool {
    source.contains("Stub IR")
}

fn load_ir_file(path: &Path) -> Option<IrSidecar> {
    let source = std::fs::read_to_string(path).ok()?;
    // Reject labeled stubs so disk sidecars match in-memory heuristic policy.
    if is_stub_ir_text(&source) {
        return None;
    }
    let module: IrModule = parse_ir_module(&source).ok()?;
    let func = module.functions.first()?.clone();
    let blocks = block_map_from_module(&module);
    Some(IrSidecar { func, blocks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::Spanned;
    use assura_ast::{Clause, ClauseKind, ContractDecl, Decl, Expr, SourceFile};
    use assura_types::{TypeEnv, TypedFile};
    use std::sync::Arc;

    fn make_source(decls: Vec<Decl>) -> SourceFile {
        SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: decls
                .into_iter()
                .map(|d| Spanned {
                    node: d,
                    span: 0..1,
                })
                .collect(),
        }
    }

    fn typed_with_contract(name: &str) -> TypedFile {
        let source = make_source(vec![Decl::Contract(ContractDecl {
            name: name.into(),
            clauses: vec![Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::Ident("x".into())),
                effect_variables: vec![],
            }],
            fn_params: vec![],
            type_params: vec![],
        })]);
        let resolved = assura_resolve::resolve(&source).expect("resolve should succeed");
        TypedFile {
            resolved: Arc::new(resolved),
            type_env: TypeEnv::new(),
            pending_decrease_checks: vec![],
            generated_tests: vec![],
            warnings: vec![],
        }
    }

    #[test]
    fn load_ir_sidecar_by_contract_name() {
        let dir = std::env::temp_dir().join(format!("assura-ir-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ir_path = dir.join("CopyBytes.ir");
        std::fs::write(
            &ir_path,
            r#"
module copy {
  fn #0 : ($0: Bytes) -> Bytes ! pure
  {
    $result = load $0 : Bytes
  }
}
"#,
        )
        .unwrap();

        let sidecar = resolve_ir_sidecar(&[dir.as_path()], "CopyBytes").expect("should load IR");
        assert_eq!(sidecar.func.id, "#0");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn loaded_verify_extras_empty_when_no_sidecars() {
        let typed = typed_with_contract("NoIr");
        let loaded = LoadedVerifyExtras::load(std::path::Path::new("missing.assura"), &typed);
        assert!(loaded.is_empty());
        assert!(loaded.extras().is_none());
    }

    #[test]
    fn collect_job_names_includes_contract() {
        let typed = typed_with_contract("MyContract");
        let names = collect_verification_job_names(&typed);
        assert_eq!(names, vec!["MyContract".to_string()]);
    }

    #[test]
    fn from_ir_text_builds_extras() {
        let ir = "module Echo {\n  fn #0 : ($0: Int) -> Int ! pure\n  {\n    $result = load $0 : Int\n  }\n}\n";
        let loaded = LoadedVerifyExtras::from_ir_text(ir, "Echo").expect("should parse IR text");
        assert!(!loaded.is_empty(), "should have loaded IR body");
        assert_eq!(loaded.loaded_names(), vec!["Echo"]);
        let extras = loaded.extras().expect("extras should be present");
        assert!(
            extras.ir_bodies.is_some(),
            "extras should contain IR bodies"
        );
    }

    #[test]
    fn from_ir_text_rejects_invalid_ir() {
        let result = LoadedVerifyExtras::from_ir_text("not valid IR", "Foo");
        assert!(result.is_err(), "invalid IR should produce errors");
    }

    #[test]
    fn sidecar_loads_block_map_from_module() {
        let dir = std::env::temp_dir().join(format!("assura-ir-blocks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Branch.ir"),
            r#"
module branch {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $result = load $0 : Int
  }
  fn #1 : ($0: Int) -> Int ! pure
  {
    $result = const 1 : Int
  }
}
"#,
        )
        .unwrap();

        let sidecar = resolve_ir_sidecar(&[dir.as_path()], "Branch").expect("load branch IR");
        assert!(sidecar.blocks.contains_key(&1));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #273: IR sidecars loaded from disk reach parallel verification.
    #[test]
    #[cfg(feature = "z3-verify")]
    fn pipeline_loads_ir_sidecar_for_parallel_verify() {
        use crate::SolverChoice;
        use crate::VerificationCache;
        use crate::VerificationResult;
        use crate::entry::verify::verify_parallel_with_solver;
        use assura_ast::{BinOp, Clause, ClauseKind, ContractDecl, Expr, Literal};

        let dir = std::env::temp_dir().join(format!("assura-ir-pipeline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let assura_path = dir.join("CopyBytes.assura");
        std::fs::write(&assura_path, "contract CopyBytes { }\n").unwrap();
        std::fs::write(
            dir.join("CopyBytes.ir"),
            r#"
module copy {
  fn #0 : ($0: Bytes) -> Bytes ! pure
  {
    $result = load $0 : Bytes
  }
}
"#,
        )
        .unwrap();

        let raw_len_gt_zero = Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                receiver: Box::new(Spanned::no_span(Expr::Ident("raw".into()))),
                method: "length".into(),
                args: vec![],
            })),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        };
        let result_len_le_raw = Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                receiver: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
                method: "length".into(),
                args: vec![],
            })),
            op: BinOp::Lte,
            rhs: Box::new(Spanned::no_span(Expr::MethodCall {
                receiver: Box::new(Spanned::no_span(Expr::Ident("raw".into()))),
                method: "length".into(),
                args: vec![],
            })),
        };

        let source = make_source(vec![Decl::Contract(ContractDecl {
            name: "CopyBytes".into(),
            clauses: vec![
                Clause {
                    kind: ClauseKind::Input,
                    body: Spanned::no_span(Expr::Raw(vec![
                        "raw".into(),
                        ":".into(),
                        "Bytes".into(),
                    ])),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Output,
                    body: Spanned::no_span(Expr::Raw(vec![
                        "result".into(),
                        ":".into(),
                        "Bytes".into(),
                    ])),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Requires,
                    body: Spanned::no_span(raw_len_gt_zero),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Spanned::no_span(result_len_le_raw),
                    effect_variables: vec![],
                },
            ],
            fn_params: vec![],
            type_params: vec![],
        })]);
        let resolved = assura_resolve::resolve(&source).expect("resolve should succeed");
        let typed = TypedFile {
            resolved: Arc::new(resolved),
            type_env: TypeEnv::new(),
            pending_decrease_checks: vec![],
            generated_tests: vec![],
            warnings: vec![],
        };

        let loaded = LoadedVerifyExtras::load(&assura_path, &typed);
        assert!(!loaded.is_empty(), "expected CopyBytes.ir to load");
        let extras = loaded.extras().expect("extras should be present");
        let cache = VerificationCache::new(&dir);
        let results = verify_parallel_with_solver(&typed, &cache, SolverChoice::Z3, Some(&extras));

        let ensures = results.iter().find(|r| {
            matches!(
                r,
                VerificationResult::Verified { clause_desc, .. } if clause_desc.ends_with("::ensures")
            )
        });
        assert!(
            ensures.is_some(),
            "expected verified ensures with IR sidecar, got: {results:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Missing co-located IR: analyzable ensures still verify via in-memory heuristics.
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_heuristic_ir_verifies_result_eq_param_without_sidecar() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-heuristic-ir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract Echo {
  input(x: Int)
  output(result: Int)
  ensures { result == x }
}
"#;
        let path = dir.join("echo.assura");
        std::fs::write(&path, src).unwrap();
        // Deliberately no Echo.ir on disk.
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"Echo".to_string()),
            "expected in-memory heuristic for Echo, names={:?}",
            loaded.heuristic_names()
        );
        assert!(loaded.ir_map.contains_key("Echo"));

        let results = Verifier::new(&typed).source(&path).verify();
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => clause_desc.ends_with("::ensures"),
        });
        assert!(
            matches!(ensures, Some(VerificationResult::Verified { .. })),
            "result == x should verify via synthesized IR; got {results:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nested arithmetic `(x + 1) * 2` must synthesize via recursive operand temps.
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_heuristic_ir_verifies_result_eq_nested_arith_without_sidecar() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir =
            std::env::temp_dir().join(format!("assura-heuristic-nested-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract Nested {
  input(x: Int)
  output(result: Int)
  ensures { result == (x + 1) * 2 }
}
"#;
        let path = dir.join("nested.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"Nested".to_string()),
            "expected nested-arith heuristic, names={:?}",
            loaded.heuristic_names()
        );
        let results = Verifier::new(&typed).source(&path).verify();
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => clause_desc.ends_with("::ensures"),
        });
        assert!(
            matches!(ensures, Some(VerificationResult::Verified { .. })),
            "nested arith should verify; got {results:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Unary negation `result == -x` synthesizes as `0 - x`.
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_heuristic_ir_verifies_result_eq_neg_x_without_sidecar() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-heuristic-neg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract Neg {
  input(x: Int)
  output(result: Int)
  ensures { result == -x }
}
"#;
        let path = dir.join("neg.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"Neg".to_string()),
            "expected neg heuristic, names={:?}",
            loaded.heuristic_names()
        );
        let results = Verifier::new(&typed).source(&path).verify();
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => clause_desc.ends_with("::ensures"),
        });
        assert!(
            matches!(ensures, Some(VerificationResult::Verified { .. })),
            "result == -x should verify; got {results:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Param + literal arithmetic (`result == x + 1`) must synthesize, not stub.
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_heuristic_ir_verifies_result_eq_x_plus_one_without_sidecar() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-heuristic-inc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract Inc {
  input(x: Int)
  output(result: Int)
  ensures { result == x + 1 }
}
"#;
        let path = dir.join("inc.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"Inc".to_string()),
            "expected in-memory heuristic for Inc (param+literal arith), names={:?}",
            loaded.heuristic_names()
        );

        let results = Verifier::new(&typed).source(&path).verify();
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => clause_desc.ends_with("::ensures"),
        });
        assert!(
            matches!(ensures, Some(VerificationResult::Verified { .. })),
            "result == x + 1 should verify via synthesized IR; got {results:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Disk sidecars that are only labeled stubs must not load as real IR.
    #[test]
    fn stub_ir_text_on_disk_is_not_loaded_as_implementation() {
        let dir = std::env::temp_dir().join(format!("assura-stub-disk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = r#"
contract Pos {
  input(x: Int)
  output(result: Int)
  ensures { result > 0 }
}
"#;
        let path = dir.join("pos.assura");
        std::fs::write(&path, src).unwrap();
        // Identity stub (historical --write-ir poison): would wrongly constrain result.
        let stub = r#"// Stub IR for Pos — AI replaces body to satisfy contract ensures
module Pos {
fn #0 : ($0: Int) -> Int ! pure
{
    $result = load $0 : Int
}
}
"#;
        std::fs::write(dir.join("Pos.ir"), stub).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load(&path, &typed);
        assert!(
            !loaded.ir_map.contains_key("Pos"),
            "stub co-located IR must not load as implementation"
        );
        assert!(is_stub_ir_text(stub));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_heuristic_ir_verifies_result_eq_abs_x_without_sidecar() {
        use crate::VerificationResult;
        use crate::Verifier;
        let dir = std::env::temp_dir().join(format!("assura-abs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = r#"
contract AbsX {
  input(x: Int)
  output(result: Int)
  ensures { result == abs(x) }
}
"#;
        let path = dir.join("abs.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"AbsX".to_string()),
            "expected abs heuristic, names={:?}",
            loaded.heuristic_names()
        );
        let results = Verifier::new(&typed).source(&path).verify();
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => clause_desc.ends_with("::ensures"),
        });
        assert!(
            matches!(ensures, Some(VerificationResult::Verified { .. })),
            "result == abs(x) should verify; got {results:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_heuristic_ir_verifies_bool_comparison_without_sidecar() {
        use crate::VerificationResult;
        use crate::Verifier;
        let dir = std::env::temp_dir().join(format!("assura-bool-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = r#"
contract Pos {
  input(x: Int)
  output(result: Bool)
  ensures { result == (x > 0) }
}
"#;
        let path = dir.join("pos.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"Pos".to_string()),
            "expected bool cmp heuristic, names={:?}",
            loaded.heuristic_names()
        );
        let results = Verifier::new(&typed).source(&path).verify();
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => clause_desc.ends_with("::ensures"),
        });
        assert!(
            matches!(ensures, Some(VerificationResult::Verified { .. })),
            "result == (x > 0) should verify; got {results:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Multi-arg arithmetic ensures synthesize IR and verify without a sidecar.
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_heuristic_ir_verifies_result_eq_x_plus_y_without_sidecar() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-heuristic-add-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract Add {
  input(x: Int, y: Int)
  output(result: Int)
  ensures { result == x + y }
}
"#;
        let path = dir.join("add.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"Add".to_string()),
            "expected in-memory heuristic for Add, names={:?}",
            loaded.heuristic_names()
        );

        let results = Verifier::new(&typed).source(&path).verify();
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => clause_desc.ends_with("::ensures"),
        });
        assert!(
            matches!(ensures, Some(VerificationResult::Verified { .. })),
            "result == x + y should verify via synthesized IR; got {results:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Bool match + BoolZeroOrOne prelude so free Int cannot assign x=2.
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_match_bool_heuristic_verifies() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-match-bool-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract MatchBool {
  input(x: Bool)
  output(result: Int)
  ensures { result == match x { true => 1, false => 0 } }
}
"#;
        let path = dir.join("match_bool.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"MatchBool".to_string()),
            "expected heuristic for MatchBool, names={:?}",
            loaded.heuristic_names()
        );

        let results = Verifier::new(&typed).source(&path).verify();
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => {
                clause_desc.starts_with("MatchBool") && clause_desc.ends_with("::ensures")
            }
        });
        assert!(
            matches!(ensures, Some(VerificationResult::Verified { .. })),
            "Bool match ensures should verify; got {results:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `ensures { result == min(x, y) }` / `max(x, y)` via if-compare synthesis.
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_min_max_call_heuristic_verifies() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-min-max-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract MinC {
  input(x: Int, y: Int)
  output(result: Int)
  ensures { result == min(x, y) }
}
contract MaxC {
  input(x: Int, y: Int)
  output(result: Int)
  ensures { result == max(x, y) }
}
"#;
        let path = dir.join("minmax.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"MinC".to_string())
                && loaded.heuristic_names().contains(&"MaxC".to_string()),
            "expected heuristics for MinC/MaxC, names={:?}",
            loaded.heuristic_names()
        );

        let results = Verifier::new(&typed).source(&path).verify();
        for name in ["MinC", "MaxC"] {
            let ensures = results.iter().find(|r| match r {
                VerificationResult::Verified { clause_desc, .. }
                | VerificationResult::Counterexample { clause_desc, .. }
                | VerificationResult::Unknown { clause_desc, .. }
                | VerificationResult::Timeout { clause_desc } => {
                    clause_desc.starts_with(name) && clause_desc.ends_with("::ensures")
                }
            });
            assert!(
                matches!(ensures, Some(VerificationResult::Verified { .. })),
                "{name} ensures should verify via min/max IR; got {results:?}"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `if x > 0 && y > 0 then …` must materialize logical And in the condition.
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_if_and_condition_heuristic_verifies() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-if-and-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract IfAnd {
  input(x: Int, y: Int)
  output(result: Int)
  ensures { result == if x > 0 && y > 0 then 1 else 0 }
}
"#;
        let path = dir.join("if_and.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"IfAnd".to_string()),
            "expected heuristic for IfAnd, names={:?}",
            loaded.heuristic_names()
        );

        let results = Verifier::new(&typed).source(&path).verify();
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => {
                clause_desc.starts_with("IfAnd") && clause_desc.ends_with("::ensures")
            }
        });
        assert!(
            matches!(ensures, Some(VerificationResult::Verified { .. })),
            "if with && condition should verify; got {results:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nested if ensures should synthesize multi-block IR and verify (#885).
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_nested_if_heuristic_verifies() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-nested-if-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract Nested {
  input(x: Int)
  output(result: Int)
  ensures { result == if x > 0 then (if x > 10 then 2 else 1) else 0 }
}
"#;
        let path = dir.join("nested.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"Nested".to_string()),
            "expected heuristic for Nested, names={:?}",
            loaded.heuristic_names()
        );

        let results = Verifier::new(&typed).source(&path).verify();
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => {
                clause_desc.starts_with("Nested") && clause_desc.ends_with("::ensures")
            }
        });
        assert!(
            matches!(ensures, Some(VerificationResult::Verified { .. })),
            "nested if ensures should verify via synthesized IR; got {results:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `ensures { result == !x }` and `result == (x && y)` via bool logic synthesis.
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_bool_logic_heuristic_verifies() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-bool-logic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract NotB {
  input(x: Bool)
  output(result: Bool)
  ensures { result == !x }
}
contract AndB {
  input(x: Bool, y: Bool)
  output(result: Bool)
  ensures { result == (x && y) }
}
"#;
        let path = dir.join("bool_logic.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"NotB".to_string())
                && loaded.heuristic_names().contains(&"AndB".to_string()),
            "expected heuristics for NotB/AndB, names={:?}",
            loaded.heuristic_names()
        );

        let results = Verifier::new(&typed).source(&path).verify();
        for name in ["NotB", "AndB"] {
            let ensures = results.iter().find(|r| match r {
                VerificationResult::Verified { clause_desc, .. }
                | VerificationResult::Counterexample { clause_desc, .. }
                | VerificationResult::Unknown { clause_desc, .. }
                | VerificationResult::Timeout { clause_desc } => {
                    clause_desc.starts_with(name) && clause_desc.ends_with("::ensures")
                }
            });
            assert!(
                matches!(ensures, Some(VerificationResult::Verified { .. })),
                "{name} ensures should verify via synthesized IR; got {results:?}"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `ensures { result == match x { 0 => 0, _ => 1 } }` must verify via
    /// synthesized match IR (pattern equality, not bare ite_nonzero).
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_match_int_heuristic_verifies() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-match-ir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract MatchInt {
  input(x: Int)
  output(result: Int)
  ensures { result == match x { 0 => 0, _ => 1 } }
}
"#;
        let path = dir.join("match_int.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            loaded.heuristic_names().contains(&"MatchInt".to_string()),
            "expected in-memory heuristic for MatchInt, names={:?}",
            loaded.heuristic_names()
        );

        let results = Verifier::new(&typed).source(&path).verify();
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => clause_desc.ends_with("::ensures"),
        });
        assert!(
            matches!(ensures, Some(VerificationResult::Verified { .. })),
            "match int ensures should verify via synthesized IR; got {results:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Unanalyzable ensures must not get a silent identity heuristic (stay Unknown).
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_unanalyzable_result_ensures_stays_unknown_without_fake_identity() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-no-fake-ir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract Positive {
  input(x: Int)
  output(result: Int)
  ensures { result > 0 }
}
"#;
        let path = dir.join("pos.assura");
        std::fs::write(&path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let loaded = LoadedVerifyExtras::load_or_synthesize(&path, &typed);
        assert!(
            !loaded.ir_map.contains_key("Positive"),
            "must not inject stub identity for unanalyzable ensures"
        );

        let results = Verifier::new(&typed).source(&path).verify();
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => {
                clause_desc.starts_with("Positive") && clause_desc.ends_with("::ensures")
            }
        });
        assert!(
            matches!(ensures, Some(VerificationResult::Unknown { .. })),
            "unanalyzable result ensures should be Unknown, not CE/Verified; got {results:?}"
        );
        if let Some(VerificationResult::Unknown { reason, .. }) = ensures {
            assert!(
                reason.contains("not auto-synthesizable") || reason.contains("unconstrained"),
                "reason should mention synthesizable/unconstrained path: {reason}"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #863: same-file unary pure callee yields non-identity call IR and Z3 proof.
    ///
    /// Generation uses call-shaped ensures (`result == double(x)`). Verification
    /// uses a concrete numeric postcondition (`result == 6` under `x == 3`) so
    /// the ensures encoder does not need interprocedural `Call` equating; the
    /// IR still implements the call via inlined `double` body (x+x).
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_call_shaped_ir_verifies_with_in_file_callee() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-call-ir-863-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Shape that exercises the call planner (not const/identity).
        let gen_src = r#"
contract double {
  input(x: Int)
  output(result: Int)
  ensures { result == x + x }
}
contract UseDouble {
  input(x: Int)
  output(result: Int)
  ensures { result == double(x) }
}
"#;
        let typed_gen = crate::test_util::typecheck_ok(gen_src);
        let stubs = stub_ir_sidecars_for_typed(&typed_gen);

        let use_ir = stubs
            .get("UseDouble")
            .expect("UseDouble IR should be generated");
        assert!(
            use_ir.contains("call double"),
            "caller IR should call double:\n{use_ir}"
        );
        assert!(
            use_ir.contains("arith add $0 $0"),
            "sibling must not be identity (#863):\n{use_ir}"
        );
        let double_ir = stubs.get("double").expect("double IR should be generated");
        assert!(
            double_ir.contains("arith add $0 $0"),
            "double body from its own ensures:\n{double_ir}"
        );

        for (name, text) in &stubs {
            std::fs::write(dir.join(format!("{name}.ir")), text).unwrap();
        }

        // Prove the call path: double(3)=6 under co-located IR bodies.
        let prove_src = r#"
contract double {
  input(x: Int)
  output(result: Int)
  ensures { result == x + x }
}
contract UseDouble {
  input(x: Int)
  output(result: Int)
  requires { x == 3 }
  ensures { result == 6 }
}
"#;
        let assura_path = dir.join("call_double.assura");
        std::fs::write(&assura_path, prove_src).unwrap();
        let typed = crate::test_util::typecheck_ok(prove_src);

        let results = Verifier::new(&typed).source(&assura_path).verify();
        let use_ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => {
                clause_desc.starts_with("UseDouble") && clause_desc.ends_with("::ensures")
            }
        });
        assert!(
            matches!(use_ensures, Some(VerificationResult::Verified { .. })),
            "UseDouble with call-inlined double IR should verify; got: {results:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ensures-side call equating: `result == double(x)` verifies when double's
    /// functional ensures is in-file and both IR sidecars are co-located.
    #[test]
    #[cfg(feature = "z3-verify")]
    fn e2e_result_eq_double_x_verifies_with_callee_spec() {
        use crate::VerificationResult;
        use crate::Verifier;

        let dir = std::env::temp_dir().join(format!("assura-call-eq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = r#"
contract double {
  input(x: Int)
  output(result: Int)
  ensures { result == x + x }
}
contract UseDouble {
  input(x: Int)
  output(result: Int)
  ensures { result == double(x) }
}
"#;
        let assura_path = dir.join("call_eq.assura");
        std::fs::write(&assura_path, src).unwrap();
        let typed = crate::test_util::typecheck_ok(src);
        let stubs = stub_ir_sidecars_for_typed(&typed);
        for (name, text) in &stubs {
            std::fs::write(dir.join(format!("{name}.ir")), text).unwrap();
        }

        let results = Verifier::new(&typed).source(&assura_path).verify();
        let use_ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc } => {
                clause_desc.starts_with("UseDouble") && clause_desc.ends_with("::ensures")
            }
        });
        assert!(
            matches!(use_ensures, Some(VerificationResult::Verified { .. })),
            "result == double(x) should verify via callee functional ensures + IR; got: {results:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
