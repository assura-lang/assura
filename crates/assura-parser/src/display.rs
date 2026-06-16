//! AST display and formatting utilities.
//!
//! Provides human-readable printing of the Assura AST, expression-to-string
//! conversion, and helper functions for CLI output formatting.

use crate::ast::*;

/// Print a `SourceFile` AST in a human-readable tree format.
pub fn print_ast(file: &SourceFile) {
    if let Some(p) = &file.project {
        println!("Project: {} [{}]", p.name, p.profile.join(", "));
    }
    if let Some(m) = &file.module {
        println!("Module: {}", m.path.join("."));
    }
    for imp in &file.imports {
        let alias = imp
            .alias
            .as_deref()
            .map(|a| format!(" as {a}"))
            .unwrap_or_default();
        let items = if imp.items.is_empty() {
            String::new()
        } else {
            format!(" {{{}}}", imp.items.join(", "))
        };
        println!("Import: {}{alias}{items}", imp.path.join("."));
    }
    for d in &file.decls {
        print_decl(&d.node, 0);
    }
}

/// Print a single declaration at a given indentation level.
pub(crate) fn print_decl(decl: &Decl, indent: usize) {
    let pad = "  ".repeat(indent);
    match decl {
        Decl::Contract(c) => {
            let tps = if c.type_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", c.type_params.join(", "))
            };
            println!("{pad}Contract: {}{tps}", c.name);
            for cl in &c.clauses {
                let body = truncate(&expr_to_string(&cl.body), 60);
                println!("{pad}  {:?}: {body}", cl.kind);
            }
        }
        Decl::TypeDef(t) => {
            let tps = if t.type_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", t.type_params.join(", "))
            };
            match &t.body {
                TypeBody::Refined(toks) => {
                    println!(
                        "{pad}Type: {}{tps} = {{{}}}",
                        t.name,
                        truncate(&toks.join(" "), 50)
                    );
                }
                TypeBody::Alias(toks) => {
                    println!(
                        "{pad}Type: {}{tps} = {}",
                        t.name,
                        truncate(&toks.join(" "), 50)
                    );
                }
                TypeBody::Struct(fields) => {
                    println!("{pad}Type: {}{tps}", t.name);
                    for f in fields {
                        let pub_str = if f.is_pub { "pub " } else { "" };
                        println!("{pad}  {pub_str}{}: {}", f.name, f.ty.join(" "));
                    }
                }
                TypeBody::Empty => println!("{pad}Type: {}{tps}", t.name),
            }
        }
        Decl::EnumDef(e) => {
            let tps = if e.type_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", e.type_params.join(", "))
            };
            println!("{pad}Enum: {}{tps}", e.name);
            for v in &e.variants {
                if v.fields.is_empty() {
                    println!("{pad}  {}", v.name);
                } else {
                    println!("{pad}  {}({})", v.name, v.fields.join(" "));
                }
            }
        }
        Decl::Extern(ex) => {
            let params = ex
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.ty.join(" ")))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "{pad}Extern: fn {}({params}) -> {}",
                ex.name,
                ex.return_ty.join(" ")
            );
            for cl in &ex.clauses {
                println!(
                    "{pad}  {:?}: {}",
                    cl.kind,
                    truncate(&expr_to_string(&cl.body), 50)
                );
            }
        }
        Decl::Bind(b) => {
            let params = b
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.ty.join(" ")))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "{pad}Bind: \"{}\" as {}({params}) -> {}",
                b.target_path,
                b.name,
                b.return_ty.join(" ")
            );
            for cl in &b.clauses {
                println!(
                    "{pad}  {:?}: {}",
                    cl.kind,
                    truncate(&expr_to_string(&cl.body), 50)
                );
            }
        }
        Decl::Prophecy(p) => {
            println!("{pad}GhostProphecy: {}: {}", p.name, p.ty_tokens.join(" "));
        }
        Decl::FnDef(f) => {
            let params = f
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.ty.join(" ")))
                .collect::<Vec<_>>()
                .join(", ");
            let ret = if f.return_ty.is_empty() {
                String::new()
            } else {
                format!(" -> {}", f.return_ty.join(" "))
            };
            println!("{pad}Fn: {}({params}){ret}", f.name);
            for cl in &f.clauses {
                println!(
                    "{pad}  {:?}: {}",
                    cl.kind,
                    truncate(&expr_to_string(&cl.body), 50)
                );
            }
        }
        Decl::Service(s) => {
            println!("{pad}Service: {}", s.name);
            for item in &s.items {
                match item {
                    ServiceItem::TypeDef(t) => {
                        println!("{pad}  type: {}", t.name);
                    }
                    ServiceItem::States(states) => {
                        println!("{pad}  states: {}", states.join(" -> "));
                    }
                    ServiceItem::Operation { name, clauses } => {
                        println!("{pad}  operation: {name}");
                        for cl in clauses {
                            println!(
                                "{pad}    {:?}: {}",
                                cl.kind,
                                truncate(&expr_to_string(&cl.body), 40)
                            );
                        }
                    }
                    ServiceItem::Query { name, clauses } => {
                        println!("{pad}  query: {name}");
                        for cl in clauses {
                            println!(
                                "{pad}    {:?}: {}",
                                cl.kind,
                                truncate(&expr_to_string(&cl.body), 40)
                            );
                        }
                    }
                    ServiceItem::Invariant(expr) => {
                        println!("{pad}  invariant: {}", truncate(&expr_to_string(expr), 50));
                    }
                    _ => {}
                }
            }
        }
        Decl::CodecRegistry(cr) => {
            println!(
                "{pad}CodecRegistry: {} (output: {}, {} codec(s))",
                cr.name,
                cr.output_type.join(" "),
                cr.codecs.len()
            );
        }
        Decl::Block {
            kind, name, body, ..
        } => {
            println!("{pad}{kind}: {name} ({} clause(s))", body.len());
        }
    }
}

/// Convert an `Expr` to a human-readable string representation.
pub fn expr_to_string(expr: &Expr) -> String {
    match expr {
        Expr::Literal(lit) => match lit {
            Literal::Int(s) | Literal::Float(s) => s.clone(),
            Literal::Str(s) => format!("\"{s}\""),
            Literal::Bool(b) => b.to_string(),
        },
        Expr::Ident(s) => s.clone(),
        Expr::Field(e, f) => format!("{}.{f}", expr_to_string(e)),
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let args_s: Vec<String> = args.iter().map(expr_to_string).collect();
            format!(
                "{}.{method}({})",
                expr_to_string(receiver),
                args_s.join(", ")
            )
        }
        Expr::Call { func, args } => {
            let args_s: Vec<String> = args.iter().map(expr_to_string).collect();
            format!("{}({})", expr_to_string(func), args_s.join(", "))
        }
        Expr::Index { expr: e, index } => {
            format!("{}[{}]", expr_to_string(e), expr_to_string(index))
        }
        Expr::BinOp { lhs, op, rhs } => {
            let op_s = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Mod => "mod",
                BinOp::Eq => "==",
                BinOp::Neq => "!=",
                BinOp::Lt => "<",
                BinOp::Lte => "<=",
                BinOp::Gt => ">",
                BinOp::Gte => ">=",
                BinOp::And => "and",
                BinOp::Or => "or",
                BinOp::Implies => "=>",
                BinOp::In => "in",
                BinOp::NotIn => "not in",
                BinOp::Concat => "++",
                BinOp::Range => "..",
            };
            format!("{} {op_s} {}", expr_to_string(lhs), expr_to_string(rhs))
        }
        Expr::UnaryOp { op, expr: e } => {
            let op_s = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "not",
            };
            format!("{op_s} {}", expr_to_string(e))
        }
        Expr::Old(e) => format!("old({})", expr_to_string(e)),
        Expr::Forall { var, domain, body } => {
            format!(
                "forall {var} in {}: {}",
                expr_to_string(domain),
                expr_to_string(body)
            )
        }
        Expr::Exists { var, domain, body } => {
            format!(
                "exists {var} in {}: {}",
                expr_to_string(domain),
                expr_to_string(body)
            )
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => match else_branch {
            Some(eb) => format!(
                "if {} then {} else {}",
                expr_to_string(cond),
                expr_to_string(then_branch),
                expr_to_string(eb)
            ),
            None => format!(
                "if {} then {}",
                expr_to_string(cond),
                expr_to_string(then_branch)
            ),
        },
        Expr::Paren(e) => format!("({})", expr_to_string(e)),
        Expr::List(elems) => {
            let elems_s: Vec<String> = elems.iter().map(expr_to_string).collect();
            format!("[{}]", elems_s.join(", "))
        }
        Expr::Cast { expr: e, ty } => format!("{} as {ty}", expr_to_string(e)),
        Expr::Block(exprs) => {
            let strs: Vec<String> = exprs.iter().map(expr_to_string).collect();
            strs.join(" ")
        }
        Expr::Ghost(inner) => format!("ghost {{ {} }}", expr_to_string(inner)),
        Expr::Apply { lemma_name, args } => {
            let args_s: Vec<String> = args.iter().map(expr_to_string).collect();
            format!("apply {lemma_name}({})", args_s.join(", "))
        }
        Expr::Match { scrutinee, arms } => {
            let scrut = expr_to_string(scrutinee);
            let arms_s: Vec<String> = arms
                .iter()
                .map(|arm| {
                    let pat = match &arm.pattern {
                        Pattern::Ident(name) => name.clone(),
                        Pattern::Wildcard => "_".into(),
                        Pattern::Literal(lit) => format!("{lit:?}"),
                        Pattern::Constructor { name, fields } => {
                            let fs: Vec<String> = fields
                                .iter()
                                .map(|f| match f {
                                    Pattern::Ident(n) => n.clone(),
                                    Pattern::Wildcard => "_".into(),
                                    other => format!("{other:?}"),
                                })
                                .collect();
                            format!("{name}({})", fs.join(", "))
                        }
                        Pattern::Tuple(pats) => {
                            let ps: Vec<String> = pats
                                .iter()
                                .map(|p| match p {
                                    Pattern::Ident(n) => n.clone(),
                                    Pattern::Wildcard => "_".into(),
                                    other => format!("{other:?}"),
                                })
                                .collect();
                            format!("({})", ps.join(", "))
                        }
                    };
                    format!("{pat} => {}", expr_to_string(&arm.body))
                })
                .collect();
            format!("match {scrut} {{ {} }}", arms_s.join(", "))
        }
        Expr::Let { name, value, body } => {
            format!(
                "let {} = {} in {}",
                name,
                expr_to_string(value),
                expr_to_string(body)
            )
        }
        Expr::Tuple(elems) => {
            let items: Vec<String> = elems.iter().map(expr_to_string).collect();
            format!("({})", items.join(", "))
        }
        Expr::Raw(tokens) => tokens.join(" "),
    }
}

/// Truncate a string to `max` characters, appending `...` if truncated.
///
/// Uses char boundaries to avoid panics on multi-byte UTF-8 strings.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let end = s.char_indices().nth(max).map_or(s.len(), |(idx, _)| idx);
        format!("{}...", &s[..end])
    } else {
        s.to_string()
    }
}
