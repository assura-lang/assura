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
                if cl.effect_variables.is_empty() {
                    println!("{pad}  {:?}: {body}", cl.kind);
                } else {
                    println!(
                        "{pad}  {:?}: {body} [effect_variables: {}]",
                        cl.kind,
                        cl.effect_variables.join(", ")
                    );
                }
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
        Expr::BinOp { .. } => {
            // Iteratively walk left-leaning BinOp chains to avoid stack
            // overflow on deeply nested operator expressions.
            let mut parts: Vec<String> = Vec::new();
            let mut cur = expr;
            loop {
                match cur {
                    Expr::BinOp { lhs, op, rhs } => {
                        let op_s = binop_str(op);
                        parts.push(format!(" {op_s} {}", expr_to_string(rhs)));
                        cur = lhs;
                    }
                    _ => {
                        parts.push(expr_to_string(cur));
                        break;
                    }
                }
            }
            parts.reverse();
            parts.concat()
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

/// Map a `BinOp` to its string representation.
fn binop_str(op: &BinOp) -> &'static str {
    match op {
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

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncate_ascii_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_ascii_exact() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_ascii_long() {
        assert_eq!(truncate("hello world", 5), "hello...");
    }

    #[test]
    fn truncate_multibyte_utf8() {
        // Each emoji is 4 bytes; truncating at char boundary 2 must not
        // panic by slicing inside a multi-byte sequence.
        let s = "🦀🔥🎉";
        let result = truncate(s, 2);
        assert_eq!(result, "🦀🔥...");
    }

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn truncate_zero_max() {
        assert_eq!(truncate("abc", 0), "...");
    }

    // ---- expr_to_string tests ----

    use super::expr_to_string;
    use crate::ast::*;

    #[test]
    fn expr_to_string_int_literal() {
        assert_eq!(
            expr_to_string(&Expr::Literal(Literal::Int("42".into()))),
            "42"
        );
    }

    #[test]
    fn expr_to_string_float_literal() {
        assert_eq!(
            expr_to_string(&Expr::Literal(Literal::Float("3.14".into()))),
            "3.14"
        );
    }

    #[test]
    fn expr_to_string_str_literal() {
        assert_eq!(
            expr_to_string(&Expr::Literal(Literal::Str("hello".into()))),
            "\"hello\""
        );
    }

    #[test]
    fn expr_to_string_bool_literal() {
        assert_eq!(expr_to_string(&Expr::Literal(Literal::Bool(true))), "true");
        assert_eq!(
            expr_to_string(&Expr::Literal(Literal::Bool(false))),
            "false"
        );
    }

    #[test]
    fn expr_to_string_ident() {
        assert_eq!(expr_to_string(&Expr::Ident("x".into())), "x");
    }

    #[test]
    fn expr_to_string_field() {
        let e = Expr::Field(Box::new(Expr::Ident("point".into())), "x".into());
        assert_eq!(expr_to_string(&e), "point.x");
    }

    #[test]
    fn expr_to_string_nested_field() {
        let e = Expr::Field(
            Box::new(Expr::Field(Box::new(Expr::Ident("a".into())), "b".into())),
            "c".into(),
        );
        assert_eq!(expr_to_string(&e), "a.b.c");
    }

    #[test]
    fn expr_to_string_method_call() {
        let e = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("list".into())),
            method: "push".into(),
            args: vec![Expr::Literal(Literal::Int("1".into()))],
        };
        assert_eq!(expr_to_string(&e), "list.push(1)");
    }

    #[test]
    fn expr_to_string_method_call_no_args() {
        let e = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("buf".into())),
            method: "len".into(),
            args: vec![],
        };
        assert_eq!(expr_to_string(&e), "buf.len()");
    }

    #[test]
    fn expr_to_string_call() {
        let e = Expr::Call {
            func: Box::new(Expr::Ident("max".into())),
            args: vec![Expr::Ident("a".into()), Expr::Ident("b".into())],
        };
        assert_eq!(expr_to_string(&e), "max(a, b)");
    }

    #[test]
    fn expr_to_string_index() {
        let e = Expr::Index {
            expr: Box::new(Expr::Ident("arr".into())),
            index: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        assert_eq!(expr_to_string(&e), "arr[0]");
    }

    #[test]
    fn expr_to_string_binop() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Ident("x".into())),
            op: BinOp::Add,
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        };
        assert_eq!(expr_to_string(&e), "x + 1");
    }

    #[test]
    fn expr_to_string_chained_binop() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::BinOp {
                lhs: Box::new(Expr::Ident("a".into())),
                op: BinOp::Add,
                rhs: Box::new(Expr::Ident("b".into())),
            }),
            op: BinOp::Mul,
            rhs: Box::new(Expr::Ident("c".into())),
        };
        assert_eq!(expr_to_string(&e), "a + b * c");
    }

    #[test]
    fn expr_to_string_unary_neg() {
        let e = Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: Box::new(Expr::Ident("x".into())),
        };
        assert_eq!(expr_to_string(&e), "- x");
    }

    #[test]
    fn expr_to_string_unary_not() {
        let e = Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: Box::new(Expr::Literal(Literal::Bool(true))),
        };
        assert_eq!(expr_to_string(&e), "not true");
    }

    #[test]
    fn expr_to_string_old() {
        let e = Expr::Old(Box::new(Expr::Ident("counter".into())));
        assert_eq!(expr_to_string(&e), "old(counter)");
    }

    #[test]
    fn expr_to_string_forall() {
        let e = Expr::Forall {
            var: "i".into(),
            domain: Box::new(Expr::Ident("items".into())),
            body: Box::new(Expr::BinOp {
                lhs: Box::new(Expr::Ident("i".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            }),
        };
        assert_eq!(expr_to_string(&e), "forall i in items: i > 0");
    }

    #[test]
    fn expr_to_string_exists() {
        let e = Expr::Exists {
            var: "x".into(),
            domain: Box::new(Expr::Ident("set".into())),
            body: Box::new(Expr::Literal(Literal::Bool(true))),
        };
        assert_eq!(expr_to_string(&e), "exists x in set: true");
    }

    #[test]
    fn expr_to_string_if_with_else() {
        let e = Expr::If {
            cond: Box::new(Expr::Ident("flag".into())),
            then_branch: Box::new(Expr::Literal(Literal::Int("1".into()))),
            else_branch: Some(Box::new(Expr::Literal(Literal::Int("0".into())))),
        };
        assert_eq!(expr_to_string(&e), "if flag then 1 else 0");
    }

    #[test]
    fn expr_to_string_if_no_else() {
        let e = Expr::If {
            cond: Box::new(Expr::Ident("cond".into())),
            then_branch: Box::new(Expr::Literal(Literal::Bool(true))),
            else_branch: None,
        };
        assert_eq!(expr_to_string(&e), "if cond then true");
    }

    #[test]
    fn expr_to_string_paren() {
        let e = Expr::Paren(Box::new(Expr::Ident("x".into())));
        assert_eq!(expr_to_string(&e), "(x)");
    }

    #[test]
    fn expr_to_string_list() {
        let e = Expr::List(vec![
            Expr::Literal(Literal::Int("1".into())),
            Expr::Literal(Literal::Int("2".into())),
            Expr::Literal(Literal::Int("3".into())),
        ]);
        assert_eq!(expr_to_string(&e), "[1, 2, 3]");
    }

    #[test]
    fn expr_to_string_empty_list() {
        assert_eq!(expr_to_string(&Expr::List(vec![])), "[]");
    }

    #[test]
    fn expr_to_string_cast() {
        let e = Expr::Cast {
            expr: Box::new(Expr::Ident("x".into())),
            ty: "Int".into(),
        };
        assert_eq!(expr_to_string(&e), "x as Int");
    }

    #[test]
    fn expr_to_string_block() {
        let e = Expr::Block(vec![Expr::Ident("a".into()), Expr::Ident("b".into())]);
        assert_eq!(expr_to_string(&e), "a b");
    }

    #[test]
    fn expr_to_string_ghost() {
        let e = Expr::Ghost(Box::new(Expr::Literal(Literal::Bool(true))));
        assert_eq!(expr_to_string(&e), "ghost { true }");
    }

    #[test]
    fn expr_to_string_apply() {
        let e = Expr::Apply {
            lemma_name: "div_pos".into(),
            args: vec![Expr::Ident("a".into()), Expr::Ident("b".into())],
        };
        assert_eq!(expr_to_string(&e), "apply div_pos(a, b)");
    }

    #[test]
    fn expr_to_string_match_wildcard() {
        let e = Expr::Match {
            scrutinee: Box::new(Expr::Ident("x".into())),
            arms: vec![MatchArm {
                pattern: Pattern::Wildcard,
                body: Expr::Literal(Literal::Int("0".into())),
            }],
        };
        assert_eq!(expr_to_string(&e), "match x { _ => 0 }");
    }

    #[test]
    fn expr_to_string_match_constructor() {
        let e = Expr::Match {
            scrutinee: Box::new(Expr::Ident("opt".into())),
            arms: vec![
                MatchArm {
                    pattern: Pattern::Constructor {
                        name: "Some".into(),
                        fields: vec![Pattern::Ident("v".into())],
                    },
                    body: Expr::Ident("v".into()),
                },
                MatchArm {
                    pattern: Pattern::Ident("None".into()),
                    body: Expr::Literal(Literal::Int("0".into())),
                },
            ],
        };
        let result = expr_to_string(&e);
        assert!(result.contains("Some(v) => v"), "got: {result}");
        assert!(result.contains("None => 0"), "got: {result}");
    }

    #[test]
    fn expr_to_string_match_tuple_pattern() {
        let e = Expr::Match {
            scrutinee: Box::new(Expr::Ident("pair".into())),
            arms: vec![MatchArm {
                pattern: Pattern::Tuple(vec![Pattern::Ident("a".into()), Pattern::Wildcard]),
                body: Expr::Ident("a".into()),
            }],
        };
        let result = expr_to_string(&e);
        assert!(result.contains("(a, _) => a"), "got: {result}");
    }

    #[test]
    fn expr_to_string_let() {
        let e = Expr::Let {
            name: "tmp".into(),
            value: Box::new(Expr::Literal(Literal::Int("5".into()))),
            body: Box::new(Expr::Ident("tmp".into())),
        };
        assert_eq!(expr_to_string(&e), "let tmp = 5 in tmp");
    }

    #[test]
    fn expr_to_string_tuple() {
        let e = Expr::Tuple(vec![
            Expr::Ident("a".into()),
            Expr::Literal(Literal::Int("1".into())),
        ]);
        assert_eq!(expr_to_string(&e), "(a, 1)");
    }

    #[test]
    fn expr_to_string_raw() {
        let e = Expr::Raw(vec!["io".into(), ".".into(), "read".into()]);
        assert_eq!(expr_to_string(&e), "io . read");
    }

    // ---- binop_str coverage ----

    use super::binop_str;

    #[test]
    fn binop_str_all_operators() {
        assert_eq!(binop_str(&BinOp::Add), "+");
        assert_eq!(binop_str(&BinOp::Sub), "-");
        assert_eq!(binop_str(&BinOp::Mul), "*");
        assert_eq!(binop_str(&BinOp::Div), "/");
        assert_eq!(binop_str(&BinOp::Mod), "mod");
        assert_eq!(binop_str(&BinOp::Eq), "==");
        assert_eq!(binop_str(&BinOp::Neq), "!=");
        assert_eq!(binop_str(&BinOp::Lt), "<");
        assert_eq!(binop_str(&BinOp::Lte), "<=");
        assert_eq!(binop_str(&BinOp::Gt), ">");
        assert_eq!(binop_str(&BinOp::Gte), ">=");
        assert_eq!(binop_str(&BinOp::And), "and");
        assert_eq!(binop_str(&BinOp::Or), "or");
        assert_eq!(binop_str(&BinOp::Implies), "=>");
        assert_eq!(binop_str(&BinOp::In), "in");
        assert_eq!(binop_str(&BinOp::NotIn), "not in");
        assert_eq!(binop_str(&BinOp::Concat), "++");
        assert_eq!(binop_str(&BinOp::Range), "..");
    }

    // ---- match pattern display edge cases ----

    #[test]
    fn expr_to_string_match_literal_pattern() {
        let e = Expr::Match {
            scrutinee: Box::new(Expr::Ident("n".into())),
            arms: vec![MatchArm {
                pattern: Pattern::Literal(Literal::Int("42".into())),
                body: Expr::Literal(Literal::Bool(true)),
            }],
        };
        let result = expr_to_string(&e);
        assert!(result.starts_with("match n {"), "got: {result}");
    }
}
