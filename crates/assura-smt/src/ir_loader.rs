//! Load Implementation IR sidecars for havoc+assume verification (#273).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use assura_parser::ast::{Decl, ServiceItem};

use crate::VerifyFileExtras;
use crate::ir::{IrFunction, IrInstr, IrModule, parse_ir_module};
use crate::ir_encode::block_map_from_module;

/// IR sidecars loaded for a source file, with a borrowed view for verification APIs.
pub struct LoadedVerifyExtras {
    pub(crate) ir_map: HashMap<String, IrFunction>,
    pub(crate) block_map: HashMap<String, HashMap<usize, Vec<IrInstr>>>,
}

impl LoadedVerifyExtras {
    /// Load `{ContractName}.ir` sidecars for all verification jobs in `typed`.
    pub fn load(source_file: &Path, typed: &assura_types::TypedFile) -> Self {
        let (ir_map, block_map) = load_ir_sidecars_for_typed(source_file, typed);
        Self { ir_map, block_map }
    }

    /// Whether any sidecar IR bodies were discovered.
    pub fn is_empty(&self) -> bool {
        self.ir_map.is_empty()
    }

    /// `Some(VerifyFileExtras)` when sidecars exist; `None` otherwise.
    pub fn extras(&self) -> Option<VerifyFileExtras<'_>> {
        (!self.ir_map.is_empty()).then_some(VerifyFileExtras {
            ir_bodies: Some(&self.ir_map),
            ir_blocks: Some(&self.block_map),
            type_env: None,
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
                            names.push(format!("{}::invariant", s.name));
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
pub fn stub_ir_sidecars_for_typed(typed: &assura_types::TypedFile) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (name, clauses, params, return_ty) in crate::entry::collect_verification_jobs(typed) {
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
            crate::ir_generate::generate_ir_sidecar_text(
                &name,
                &param_tys,
                &param_names,
                &ret,
                &clauses,
            ),
        );
    }
    out
}

fn load_ir_file(path: &Path) -> Option<IrSidecar> {
    let source = std::fs::read_to_string(path).ok()?;
    let module: IrModule = parse_ir_module(&source).ok()?;
    let func = module.functions.first()?.clone();
    let blocks = block_map_from_module(&module);
    Some(IrSidecar { func, blocks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::Spanned;
    use assura_parser::ast::{Clause, ClauseKind, ContractDecl, Decl, Expr, SourceFile};
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
                body: Expr::Ident("x".into()),
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
        use crate::entry::verify_parallel_with_solver;
        use assura_parser::ast::{BinOp, Clause, ClauseKind, ContractDecl, Expr, Literal};

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
            lhs: Box::new(Expr::MethodCall {
                receiver: Box::new(Expr::Ident("raw".into())),
                method: "length".into(),
                args: vec![],
            }),
            op: BinOp::Gt,
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        let result_len_le_raw = Expr::BinOp {
            lhs: Box::new(Expr::MethodCall {
                receiver: Box::new(Expr::Ident("result".into())),
                method: "length".into(),
                args: vec![],
            }),
            op: BinOp::Lte,
            rhs: Box::new(Expr::MethodCall {
                receiver: Box::new(Expr::Ident("raw".into())),
                method: "length".into(),
                args: vec![],
            }),
        };

        let source = make_source(vec![Decl::Contract(ContractDecl {
            name: "CopyBytes".into(),
            clauses: vec![
                Clause {
                    kind: ClauseKind::Input,
                    body: Expr::Raw(vec!["raw".into(), ":".into(), "Bytes".into()]),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Output,
                    body: Expr::Raw(vec!["result".into(), ":".into(), "Bytes".into()]),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Requires,
                    body: raw_len_gt_zero,
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: result_len_le_raw,
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
        };

        let loaded = LoadedVerifyExtras::load(&assura_path, &typed);
        assert!(!loaded.is_empty(), "expected CopyBytes.ir to load");
        let extras = loaded.extras().expect("extras should be present");
        let cache = VerificationCache::new(&dir);
        let results = verify_parallel_with_solver(&typed, &cache, SolverChoice::Z3, Some(&extras));

        let ensures = results.iter().find(|r| {
            matches!(
                r,
                VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures")
            )
        });
        assert!(
            ensures.is_some(),
            "expected verified ensures with IR sidecar, got: {results:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
