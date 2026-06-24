//! Collect SMT verification jobs from typed source (`DeclVisitor`).

use assura_ast::{
    BindDecl, BlockKind, Clause, ClauseKind, ContractDecl, DeclVisitor, ExternDecl, FnDef, Param,
    ServiceDecl, ServiceItem,
};
use assura_types::TypedFile;

use super::helpers::{extract_input_params, extract_output_return_type, type_expr_to_token_vec};

// ---------------------------------------------------------------------------
// Shared job collection (#213): eliminates duplicated Decl dispatch in
// verify_file_with_cvc5, verify_parallel_with_solver, and z3_backend.
// ---------------------------------------------------------------------------

/// A verification job: contract name, clauses, parameters, and return type.
pub(crate) type VerificationJob = (String, Vec<Clause>, Vec<Param>, Vec<String>);

/// Collect verification jobs from all declarations in a source file.
///
/// Each job is a (name, clauses, params, return_ty) tuple suitable for
/// passing to either the Z3 or CVC5 backend.
///
/// Uses [`DeclVisitor`] so new `Decl` variants only need an arm in `walk_decl`,
/// not another open-coded match here.
pub(crate) fn collect_verification_jobs(typed: &TypedFile) -> Vec<VerificationJob> {
    struct JobCollector(Vec<VerificationJob>);

    impl DeclVisitor for JobCollector {
        fn visit_contract(&mut self, c: &ContractDecl) {
            let output_ty = extract_output_return_type(&c.clauses);
            let mut input_params = extract_input_params(&c.clauses);
            input_params.extend_from_slice(&c.fn_params);
            self.0
                .push((c.name.clone(), c.clauses.clone(), input_params, output_ty));
        }
        fn visit_fn_def(&mut self, f: &FnDef) {
            self.0.push((
                f.name.clone(),
                f.clauses.clone(),
                f.params.clone(),
                type_expr_to_token_vec(f.return_ty.as_ref()),
            ));
        }
        fn visit_extern(&mut self, e: &ExternDecl) {
            self.0.push((
                e.name.clone(),
                e.clauses.clone(),
                e.params.clone(),
                type_expr_to_token_vec(e.return_ty.as_ref()),
            ));
        }
        fn visit_service(&mut self, s: &ServiceDecl) {
            for item in &s.items {
                match item {
                    ServiceItem::Operation { name, clauses } => {
                        self.0.push((
                            format!("{}.{}", s.name, name),
                            clauses.clone(),
                            vec![],
                            vec![],
                        ));
                    }
                    ServiceItem::Query { name, clauses } => {
                        self.0.push((
                            format!("{}.{}", s.name, name),
                            clauses.clone(),
                            vec![],
                            vec![],
                        ));
                    }
                    ServiceItem::Invariant(expr) => {
                        let inv_clause = Clause {
                            kind: ClauseKind::Invariant,
                            body: expr.clone(),
                            effect_variables: vec![],
                        };
                        self.0.push((
                            crate::verify_labels::invariant_desc(&s.name),
                            vec![inv_clause],
                            vec![],
                            vec![],
                        ));
                    }
                    _ => {}
                }
            }
        }
        fn visit_block(
            &mut self,
            _kind: &BlockKind,
            name: &str,
            _value: &Option<Vec<String>>,
            body: &[Clause],
        ) {
            self.0
                .push((name.to_string(), body.to_vec(), vec![], vec![]));
        }
        fn visit_bind(&mut self, b: &BindDecl) {
            self.0.push((
                b.name.clone(),
                b.clauses.clone(),
                b.params.clone(),
                type_expr_to_token_vec(b.return_ty.as_ref()),
            ));
        }
    }

    let mut collector = JobCollector(Vec::new());
    assura_ast::walk_decls(&mut collector, &typed.resolved.source.decls);
    collector.0
}
