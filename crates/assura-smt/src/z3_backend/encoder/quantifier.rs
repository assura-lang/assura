//! Quantifier encoding: domain guards, trigger patterns, bound variables.

use crate::*;
use assura_ast::SpExpr;
use assura_types::checkers::expr_references_var;
use z3::ast;

use super::Encoder;

impl Encoder {
    /// Build a domain guard for quantifier bodies.
    ///
    /// For range domains (`lo..hi`):
    /// - `is_forall=true`:  `(lo <= x && x < hi) => body`
    /// - `is_forall=false`: `(lo <= x && x < hi) && body`
    ///
    /// For non-range domains (collections, identifiers), encode
    /// membership as an uninterpreted `contains(domain, x)` predicate.
    ///
    /// Domain classification shares [`crate::encode_quantifier_policy::domain_as_range`];
    /// term construction stays Z3-local (mirrors CVC5 `guard_quantifier_body_cvc5`).
    pub(crate) fn guard_quantifier_body(
        &mut self,
        domain: &SpExpr,
        bound: &ast::Int,
        body: &ast::Bool,
        is_forall: bool,
    ) -> ast::Bool {
        if let Some((lo, hi)) = crate::encode_quantifier_policy::domain_as_range(domain) {
            let lo_val = self.encode_expr(lo).as_int(&mut self.fresh_counter);
            let hi_val = self.encode_expr(hi).as_int(&mut self.fresh_counter);
            let ge_lo = bound.ge(&lo_val);
            let lt_hi = bound.lt(&hi_val);
            let in_range = ast::Bool::and(&[&ge_lo, &lt_hi]);
            if is_forall {
                in_range.implies(body)
            } else {
                ast::Bool::and(&[&in_range, body])
            }
        } else {
            // Non-range domain: encode as uninterpreted contains(domain, x)
            let int_sort = z3::Sort::int();
            let bool_sort = z3::Sort::bool();
            let contains_fn = z3::FuncDecl::new(
                crate::encode_quantifier_policy::DOMAIN_CONTAINS_UF_NAME,
                &[&int_sort, &int_sort],
                &bool_sort,
            );
            let domain_val = self.encode_expr(domain).as_int(&mut self.fresh_counter);
            let membership = contains_fn
                .apply(&[
                    &ast::Dynamic::from_ast(&domain_val),
                    &ast::Dynamic::from_ast(bound),
                ])
                .as_bool()
                .unwrap_or_else(|| self.fresh_bool());
            if is_forall {
                membership.implies(body)
            } else {
                ast::Bool::and(&[&membership, body])
            }
        }
    }

    /// Infer Z3 trigger patterns from function calls in a quantifier body
    /// that reference the bound variable. Returns patterns for e-matching
    /// hints that help the solver instantiate quantifiers efficiently.
    pub(crate) fn infer_quantifier_patterns(
        &mut self,
        body: &SpExpr,
        bound_var: &str,
        bound_z3: &ast::Int,
    ) -> Vec<z3::Pattern> {
        let mut patterns = Vec::new();

        // Prefer AST-based trigger inference (Tier A2), then string fallback.
        if let Some(trigger) = self
            .trigger_manager
            .infer_trigger_from_expr(body, bound_var)
        {
            // Production wiring for validate_trigger: record warnings on the manager.
            let _trigger_warnings = self.trigger_manager.validate_trigger(&trigger);
            for term in &trigger.terms {
                if let Some(fname) = term.split('(').next() {
                    let int_sort = z3::Sort::int();
                    let func = z3::FuncDecl::new(fname.trim(), &[&int_sort], &int_sort);
                    let app = func.apply(&[bound_z3 as &dyn z3::ast::Ast]);
                    let pat = z3::Pattern::new(&[&app]);
                    patterns.push(pat);
                }
            }
        }

        // Direct scan: look for Call expressions that reference the bound variable
        if patterns.is_empty() {
            self.collect_trigger_calls(body, bound_var, bound_z3, &mut patterns);
        }

        patterns
    }

    /// Recursively scan an expression for function calls containing the
    /// bound variable, and create Z3 trigger patterns from them.
    pub(crate) fn collect_trigger_calls(
        &self,
        expr: &SpExpr,
        bound_var: &str,
        bound_z3: &ast::Int,
        patterns: &mut Vec<z3::Pattern>,
    ) {
        match &expr.node {
            Expr::Call { func, args } => {
                let refs_bound = args.iter().any(|a| expr_references_var(a, bound_var));
                if refs_bound && let Expr::Ident(fname) = &func.as_ref().node {
                    let int_sort = z3::Sort::int();
                    let arity = args.len();
                    let param_sorts: Vec<&z3::Sort> = (0..arity).map(|_| &int_sort).collect();
                    let func_decl = z3::FuncDecl::new(fname.as_str(), &param_sorts, &int_sort);
                    let z3_args: Vec<ast::Dynamic> = args
                        .iter()
                        .map(|a| {
                            if expr_references_var(a, bound_var) {
                                ast::Dynamic::from_ast(bound_z3)
                            } else {
                                ast::Dynamic::from_ast(&ast::Int::new_const(
                                    crate::encode_atom_policy::TRIGGER_OTHER_NAME,
                                ))
                            }
                        })
                        .collect();
                    let arg_refs: Vec<&dyn z3::ast::Ast> =
                        z3_args.iter().map(|d| d as &dyn z3::ast::Ast).collect();
                    let app = func_decl.apply(&arg_refs);
                    let pat = z3::Pattern::new(&[&app]);
                    patterns.push(pat);
                }
                for a in args {
                    self.collect_trigger_calls(a, bound_var, bound_z3, patterns);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.collect_trigger_calls(receiver, bound_var, bound_z3, patterns);
                for a in args {
                    self.collect_trigger_calls(a, bound_var, bound_z3, patterns);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.collect_trigger_calls(lhs, bound_var, bound_z3, patterns);
                self.collect_trigger_calls(rhs, bound_var, bound_z3, patterns);
            }
            Expr::UnaryOp { expr: e, .. } | Expr::Old(e) | Expr::Ghost(e) => {
                self.collect_trigger_calls(e, bound_var, bound_z3, patterns);
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.collect_trigger_calls(cond, bound_var, bound_z3, patterns);
                self.collect_trigger_calls(then_branch, bound_var, bound_z3, patterns);
                if let Some(eb) = else_branch {
                    self.collect_trigger_calls(eb, bound_var, bound_z3, patterns);
                }
            }
            Expr::Index { expr: e, index } => {
                self.collect_trigger_calls(e, bound_var, bound_z3, patterns);
                self.collect_trigger_calls(index, bound_var, bound_z3, patterns);
            }
            _ => {}
        }
    }
}
