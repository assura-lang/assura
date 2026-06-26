//! Expression-to-string display utilities.
//!
//! Provides `expr_to_string` and `truncate` for human-readable expression output.

use super::*;

pub fn expr_to_string(expr: &SpExpr) -> String {
    AssuraDisplayFolder.fold_expr(expr)
}

/// `ExprFolder` implementation that produces Assura source text.
struct AssuraDisplayFolder;

impl ExprFolder for AssuraDisplayFolder {
    type Output = String;

    fn fold_literal(&mut self, lit: &Literal) -> String {
        literal_to_string(lit)
    }

    fn fold_ident(&mut self, name: &str) -> String {
        name.to_string()
    }

    fn fold_field(&mut self, base: &SpExpr, field: &str) -> String {
        format!("{}.{field}", self.fold_expr(base))
    }

    fn fold_method_call(&mut self, receiver: &SpExpr, method: &str, args: &[SpExpr]) -> String {
        format!(
            "{}.{method}({})",
            self.fold_expr(receiver),
            fold_arg_list(self, args)
        )
    }

    fn fold_call(&mut self, func: &SpExpr, args: &[SpExpr]) -> String {
        format!("{}({})", self.fold_expr(func), fold_arg_list(self, args))
    }

    fn fold_index(&mut self, base: &SpExpr, index: &SpExpr) -> String {
        format!("{}[{}]", self.fold_expr(base), self.fold_expr(index))
    }

    fn fold_binop(&mut self, lhs: &SpExpr, op: &BinOp, rhs: &SpExpr) -> String {
        // Iteratively walk left-leaning BinOp chains to avoid stack overflow.
        let mut parts: Vec<String> = Vec::new();
        let op_s = op.as_str();
        parts.push(format!(" {op_s} {}", self.fold_expr(rhs)));
        let mut cur = lhs;
        loop {
            match &cur.node {
                Expr::BinOp { lhs, op, rhs } => {
                    let op_s = op.as_str();
                    parts.push(format!(" {op_s} {}", self.fold_expr(rhs)));
                    cur = lhs;
                }
                _ => {
                    parts.push(self.fold_expr(cur));
                    break;
                }
            }
        }
        parts.reverse();
        parts.concat()
    }

    fn fold_unary_op(&mut self, op: &UnaryOp, inner: &SpExpr) -> String {
        format!("{} {}", op.as_str(), self.fold_expr(inner))
    }

    fn fold_old(&mut self, inner: &SpExpr) -> String {
        format!("old({})", self.fold_expr(inner))
    }

    fn fold_forall(&mut self, var: &str, domain: &SpExpr, body: &SpExpr) -> String {
        format!(
            "forall {var} in {}: {}",
            self.fold_expr(domain),
            self.fold_expr(body)
        )
    }

    fn fold_exists(&mut self, var: &str, domain: &SpExpr, body: &SpExpr) -> String {
        format!(
            "exists {var} in {}: {}",
            self.fold_expr(domain),
            self.fold_expr(body)
        )
    }

    fn fold_if(&mut self, cond: &SpExpr, then_br: &SpExpr, else_br: Option<&SpExpr>) -> String {
        match else_br {
            Some(eb) => format!(
                "if {} then {} else {}",
                self.fold_expr(cond),
                self.fold_expr(then_br),
                self.fold_expr(eb)
            ),
            None => format!(
                "if {} then {}",
                self.fold_expr(cond),
                self.fold_expr(then_br)
            ),
        }
    }

    fn fold_list(&mut self, items: &[SpExpr]) -> String {
        format!("[{}]", fold_joined(self, items, ", "))
    }

    fn fold_cast(&mut self, inner: &SpExpr, ty: &str) -> String {
        format!("{} as {ty}", self.fold_expr(inner))
    }

    fn fold_block(&mut self, exprs: &[SpExpr]) -> String {
        fold_joined(self, exprs, " ")
    }

    fn fold_ghost(&mut self, inner: &SpExpr) -> String {
        format!("ghost {{ {} }}", self.fold_expr(inner))
    }

    fn fold_apply(&mut self, name: &str, args: &[SpExpr]) -> String {
        format!("apply {name}({})", fold_arg_list(self, args))
    }

    fn fold_let(&mut self, name: &str, value: &SpExpr, body: &SpExpr) -> String {
        format!(
            "let {} = {} in {}",
            name,
            self.fold_expr(value),
            self.fold_expr(body)
        )
    }

    fn fold_match(&mut self, scrutinee: &SpExpr, arms: &[MatchArm]) -> String {
        let scrut = self.fold_expr(scrutinee);
        let arms_s: Vec<String> = arms
            .iter()
            .map(|arm| {
                let pat = pattern_to_display(&arm.pattern);
                format!("{pat} => {}", self.fold_expr(&arm.body))
            })
            .collect();
        format!("match {scrut} {{ {} }}", arms_s.join(", "))
    }

    fn fold_tuple(&mut self, items: &[SpExpr]) -> String {
        format!("({})", fold_joined(self, items, ", "))
    }

    fn fold_raw(&mut self, tokens: &[String]) -> String {
        tokens.join(" ")
    }
}

fn pattern_to_display(pat: &Pattern) -> String {
    match pat {
        Pattern::Ident(name) => name.clone(),
        Pattern::Wildcard => "_".into(),
        Pattern::Literal(lit) => format!("{lit:?}"),
        Pattern::Constructor { name, fields } => {
            let fs: Vec<String> = fields.iter().map(pattern_to_display).collect();
            format!("{name}({})", fs.join(", "))
        }
        Pattern::Tuple(pats) => {
            let ps: Vec<String> = pats.iter().map(pattern_to_display).collect();
            format!("({})", ps.join(", "))
        }
    }
}
/// Negate an expression at the AST level.
///
/// Applies De Morgan's laws and comparison inversion where possible,
/// avoiding the fragile string-based replacement used previously by
/// `negate_for_bmc`. Falls back to wrapping in `UnaryOp::Not` for
/// expressions that don't match a known pattern.
pub fn negate_expr(expr: &SpExpr) -> SpExpr {
    let span = expr.span.clone();
    let negated = match &expr.node {
        // De Morgan: not (a and b) => (not a) or (not b)
        Expr::BinOp {
            lhs,
            op: BinOp::And,
            rhs,
        } => Expr::BinOp {
            lhs: Box::new(negate_expr(lhs)),
            op: BinOp::Or,
            rhs: Box::new(negate_expr(rhs)),
        },
        // De Morgan: not (a or b) => (not a) and (not b)
        Expr::BinOp {
            lhs,
            op: BinOp::Or,
            rhs,
        } => Expr::BinOp {
            lhs: Box::new(negate_expr(lhs)),
            op: BinOp::And,
            rhs: Box::new(negate_expr(rhs)),
        },
        // Comparison inversion
        Expr::BinOp { lhs, op, rhs } => {
            if let Some(neg_op) = negate_comparison(op) {
                Expr::BinOp {
                    lhs: lhs.clone(),
                    op: neg_op,
                    rhs: rhs.clone(),
                }
            } else {
                // Non-invertible binop (Add, Mul, etc.): wrap in Not
                Expr::UnaryOp {
                    op: UnaryOp::Not,
                    expr: Box::new(expr.clone()),
                }
            }
        }
        // Double negation elimination: not (not e) => e
        Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: inner,
        } => return inner.as_ref().clone(),
        // Boolean literal inversion
        Expr::Literal(Literal::Bool(b)) => Expr::Literal(Literal::Bool(!b)),
        // Everything else: wrap in Not
        _ => Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: Box::new(expr.clone()),
        },
    };
    Spanned {
        node: negated,
        span,
    }
}

/// Return the negated comparison operator, if the operator is a comparison.
pub fn negate_comparison(op: &BinOp) -> Option<BinOp> {
    match op {
        BinOp::Eq => Some(BinOp::Neq),
        BinOp::Neq => Some(BinOp::Eq),
        BinOp::Lt => Some(BinOp::Gte),
        BinOp::Lte => Some(BinOp::Gt),
        BinOp::Gt => Some(BinOp::Lte),
        BinOp::Gte => Some(BinOp::Lt),
        BinOp::In => Some(BinOp::NotIn),
        BinOp::NotIn => Some(BinOp::In),
        _ => None,
    }
}

/// Truncate a string to `max` characters, appending `...` if truncated.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let end = s.char_indices().nth(max).map_or(s.len(), |(idx, _)| idx);
        format!("{}...", &s[..end])
    } else {
        s.to_string()
    }
}
