//! ADT (algebraic data type) emulation and match-pattern encoding.

use z3::ast;

use super::value::Z3Value;
use super::{AdtConstructor, AdtDef, Encoder};

impl Encoder {
    /// Register baseline ADT infrastructure used by match-pattern encoding.
    pub(crate) fn init_adt_infrastructure(&mut self) {
        if !self.adt_defs.contains_key("Option") {
            self.define_adt("Option", &[("Some", &["value"]), ("None", &[])]);
        }
    }

    /// Register a struct-like ADT for IR `field` / `construct` encoding.
    pub(crate) fn ensure_struct_adt(&mut self, type_name: &str, field_names: &[String]) {
        if field_names.is_empty() || self.adt_defs.contains_key(type_name) {
            return;
        }
        let accessors: Vec<&str> = field_names.iter().map(String::as_str).collect();
        self.define_adt(type_name, &[(type_name, accessors.as_slice())]);
    }

    /// Bind pattern variables as fresh Z3 integer constants so they
    /// are available in the arm body.
    pub(crate) fn bind_pattern_vars(
        &mut self,
        pattern: &assura_ast::Pattern,
        scrutinee: &Z3Value,
        match_adt: Option<&str>,
    ) {
        match pattern {
            assura_ast::Pattern::Ident(name) => {
                // Ident patterns in match bind the variable to the scrutinee,
                // but for SMT we use a fresh variable since we cannot always
                // decompose the scrutinee.
                if !self.vars.contains_key(name) {
                    let v = ast::Int::new_const(name.as_str());
                    self.vars.insert(name.clone(), Z3Value::Int(v));
                }
            }
            assura_ast::Pattern::Constructor { name, fields } => {
                if let (Some(adt_name), Z3Value::Int(s)) = (match_adt, scrutinee) {
                    let accessors: Vec<String> = self
                        .adt_defs
                        .get(adt_name)
                        .and_then(|def| {
                            def.constructors
                                .iter()
                                .find(|c| c.name == *name)
                                .map(|c| c.accessors.clone())
                        })
                        .unwrap_or_default();
                    for (i, field) in fields.iter().enumerate() {
                        if let assura_ast::Pattern::Ident(bind_name) = field {
                            let accessor = accessors.get(i).map(String::as_str).unwrap_or("value");
                            let val = self.adt_accessor(adt_name, accessor, s);
                            self.vars.insert(bind_name.clone(), Z3Value::Int(val));
                        } else {
                            self.bind_pattern_vars(field, scrutinee, match_adt);
                        }
                    }
                } else {
                    for field in fields {
                        self.bind_pattern_vars(field, scrutinee, match_adt);
                    }
                }
            }
            assura_ast::Pattern::Tuple(pats) => {
                for pat in pats {
                    self.bind_pattern_vars(pat, scrutinee, match_adt);
                }
            }
            assura_ast::Pattern::Wildcard | assura_ast::Pattern::Literal(_) => {}
        }
    }

    /// Register a synthetic ADT for constructor patterns in a match expression.
    pub(crate) fn register_match_adt_from_arms(
        &mut self,
        arms: &[assura_ast::MatchArm],
    ) -> Option<String> {
        let mut ctor_specs: Vec<(String, Vec<String>)> = Vec::new();
        for arm in arms {
            if let assura_ast::Pattern::Constructor { name, fields } = &arm.pattern {
                let accessors: Vec<String> = fields
                    .iter()
                    .enumerate()
                    .map(|(i, field)| match field {
                        assura_ast::Pattern::Ident(n) => n.clone(),
                        _ => format!("f{i}"),
                    })
                    .collect();
                ctor_specs.push((name.clone(), accessors));
            }
        }
        if ctor_specs.is_empty() {
            return None;
        }
        let adt_name = crate::encode_adt_policy::match_adt_fresh_name(self.fresh_counter);
        self.fresh_counter += 1;
        let accessor_refs: Vec<Vec<&str>> = ctor_specs
            .iter()
            .map(|(_, accessors)| accessors.iter().map(|s| s.as_str()).collect())
            .collect();
        let spec: Vec<(&str, &[&str])> = ctor_specs
            .iter()
            .zip(accessor_refs.iter())
            .map(|((name, _), refs)| (name.as_str(), refs.as_slice()))
            .collect();
        self.define_adt(&adt_name, &spec);
        Some(adt_name)
    }

    // -------------------------------------------------------------------
    // ADT (algebraic data type) emulation
    // -------------------------------------------------------------------

    /// Define an ADT with named constructors and their accessor fields.
    ///
    /// Each constructor gets a unique integer tag (sequential, starting at 0).
    /// The method registers:
    /// 1. A tag function `__adt_tag_<adt_name>` (uninterpreted, Int -> Int)
    /// 2. Accessor functions `__adt_<adt_name>_<field>` (uninterpreted, Int -> Int)
    /// 3. Exhaustiveness axiom: for any value x, tag(x) is one of the defined tags
    /// 4. Injectivity axioms: Ctor(a1, ..., an) == Ctor(b1, ..., bn) => ai == bi
    ///
    /// Returns the registered `AdtDef`.
    pub(crate) fn define_adt(
        &mut self,
        adt_name: &str,
        constructors: &[(&str, &[&str])],
    ) -> AdtDef {
        let mut adt_ctors = Vec::new();
        for (tag, (ctor_name, accessors)) in constructors.iter().enumerate() {
            adt_ctors.push(AdtConstructor {
                name: ctor_name.to_string(),
                tag: tag as i64,
                accessors: accessors.iter().map(|a| a.to_string()).collect(),
            });
        }
        let adt_def = AdtDef {
            name: adt_name.to_string(),
            constructors: adt_ctors,
        };

        // Register uninterpreted functions for the tag and accessors
        let tag_fn_name = crate::encode_atom_policy::adt_tag_uf_name(adt_name);
        self.make_func(&tag_fn_name, 1);

        for ctor in &adt_def.constructors {
            for accessor in &ctor.accessors {
                let acc_fn_name =
                    crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
                self.make_func(&acc_fn_name, 1);
            }
        }

        // Generate exhaustiveness axiom:
        //   forall x: tag(x) == 0 || tag(x) == 1 || ... || tag(x) == n
        let x =
            ast::Int::new_const(crate::encode_atom_policy::adt_exhaust_var_name(adt_name).as_str());
        let tag_fn = self.make_func(&tag_fn_name, 1);
        let tag_x = tag_fn
            .apply(&[&x as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());

        let tag_eqs: Vec<ast::Bool> = adt_def
            .constructors
            .iter()
            .map(|c| tag_x.eq(ast::Int::from_i64(c.tag)))
            .collect();
        let tag_eq_refs: Vec<&ast::Bool> = tag_eqs.iter().collect();
        let exhaustive = ast::Bool::or(&tag_eq_refs);
        let forall_exhaustive = ast::forall_const(&[&x as &dyn z3::ast::Ast], &[], &exhaustive);
        self.background_axioms.push(forall_exhaustive);

        // Generate injectivity axioms for each constructor with fields:
        //   forall a1..an, b1..bn:
        //     (tag(x) == TAG && acc_i(x) == ai) &&
        //     (tag(y) == TAG && acc_i(y) == bi) &&
        //     x == y
        //     => a1 == b1 && ... && an == bn
        //
        // Simplified form: for each constructor with accessors,
        //   forall x, y: x == y => acc_i(x) == acc_i(y)
        //
        // This is trivially true for UFs, so instead we encode the
        // more useful injectivity:
        //   forall x, y: (tag(x) == tag(y) == TAG &&
        //     acc_1(x) == acc_1(y) && ... && acc_n(x) == acc_n(y))
        //     => x == y
        for ctor in &adt_def.constructors {
            if ctor.accessors.is_empty() {
                // Nullary constructor: any two values with this tag are equal
                let a = ast::Int::new_const(
                    crate::encode_atom_policy::adt_inject_var_name(adt_name, &ctor.name, 'a')
                        .as_str(),
                );
                let b = ast::Int::new_const(
                    crate::encode_atom_policy::adt_inject_var_name(adt_name, &ctor.name, 'b')
                        .as_str(),
                );

                let tag_a = tag_fn
                    .apply(&[&a as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let tag_b = tag_fn
                    .apply(&[&b as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let tag_val = ast::Int::from_i64(ctor.tag);
                let both_tagged = ast::Bool::and(&[&tag_a.eq(&tag_val), &tag_b.eq(&tag_val)]);
                let eq_ab = a.eq(&b);
                let axiom = ast::forall_const(
                    &[&a as &dyn z3::ast::Ast, &b as &dyn z3::ast::Ast],
                    &[],
                    &both_tagged.implies(&eq_ab),
                );
                self.background_axioms.push(axiom);
            } else {
                // Constructor with fields: matching all accessors implies equality
                let a = ast::Int::new_const(
                    crate::encode_atom_policy::adt_inject_var_name(adt_name, &ctor.name, 'a')
                        .as_str(),
                );
                let b = ast::Int::new_const(
                    crate::encode_atom_policy::adt_inject_var_name(adt_name, &ctor.name, 'b')
                        .as_str(),
                );

                let tag_a = tag_fn
                    .apply(&[&a as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let tag_b = tag_fn
                    .apply(&[&b as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let tag_val = ast::Int::from_i64(ctor.tag);

                let mut conjuncts = vec![tag_a.eq(&tag_val), tag_b.eq(&tag_val)];
                for accessor in &ctor.accessors {
                    let acc_fn_name =
                        crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
                    let acc_fn = self.make_func(&acc_fn_name, 1);
                    let acc_a = acc_fn
                        .apply(&[&a as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let acc_b = acc_fn
                        .apply(&[&b as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    conjuncts.push(acc_a.eq(&acc_b));
                }
                let conjunct_refs: Vec<&ast::Bool> = conjuncts.iter().collect();
                let premise = ast::Bool::and(&conjunct_refs);
                let eq_ab = a.eq(&b);
                let axiom = ast::forall_const(
                    &[&a as &dyn z3::ast::Ast, &b as &dyn z3::ast::Ast],
                    &[],
                    &premise.implies(&eq_ab),
                );
                self.background_axioms.push(axiom);
            }
        }

        self.adt_defs.insert(adt_def.name.clone(), adt_def.clone());
        adt_def
    }

    /// Build a constructor application: create a fresh Int value, set its
    /// tag to the constructor's tag, and bind accessor values to the
    /// provided arguments.
    ///
    /// Tag resolution uses [`crate::encode_adt_policy::adt_ctor_tag_or_zero`]
    /// when the ADT registry is present (parity with CVC5 shell); unknown ctor
    /// still defaults to tag `0`. Accessor UF names come from
    /// [`crate::encode_atom_policy`].
    ///
    /// Returns the fresh Int representing the constructed value.
    pub(crate) fn adt_constructor(
        &mut self,
        adt_name: &str,
        ctor_name: &str,
        args: &[ast::Int],
    ) -> ast::Int {
        let adt_def = self.adt_defs.get(adt_name).cloned();
        let ctor = adt_def
            .as_ref()
            .and_then(|d| d.constructors.iter().find(|c| c.name == ctor_name));

        // Prefer sequential tag from the registered ctor; fall back to
        // name-order policy (unknown ctor → 0) when the registry entry is
        // missing or incomplete (parity with CVC5 adt_is_constructor_smtlib).
        let tag = if let Some(c) = ctor {
            c.tag
        } else if let Some(def) = adt_def.as_ref() {
            let names: Vec<&str> = def.constructors.iter().map(|c| c.name.as_str()).collect();
            crate::encode_adt_policy::adt_ctor_tag_or_zero(&names, ctor_name)
        } else {
            0
        };
        let accessors: Vec<String> = ctor.map_or_else(Vec::new, |c| c.accessors.clone());

        let val = self.fresh_int();

        // Set tag
        let tag_fn_name = crate::encode_atom_policy::adt_tag_uf_name(adt_name);
        let tag_fn = self.make_func(&tag_fn_name, 1);
        let tag_applied = tag_fn
            .apply(&[&val as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        self.background_axioms
            .push(tag_applied.eq(ast::Int::from_i64(tag)));

        // Bind accessor values
        for (i, accessor) in accessors.iter().enumerate() {
            if let Some(arg) = args.get(i) {
                let acc_fn_name =
                    crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
                let acc_fn = self.make_func(&acc_fn_name, 1);
                let acc_applied = acc_fn
                    .apply(&[&val as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                self.background_axioms.push(acc_applied.eq(arg));
            }
        }

        val
    }

    /// Test whether a value was built with a specific constructor.
    ///
    /// Returns `tag(x) == CONSTRUCTOR_TAG` as a Z3 Bool. Tag lookup mirrors
    /// [`crate::encode_adt_policy::adt_ctor_tag_or_zero`] / CVC5 shell when the
    /// ctor is not in the local registry entry.
    pub(crate) fn adt_is_constructor(
        &mut self,
        adt_name: &str,
        ctor_name: &str,
        value: &ast::Int,
    ) -> ast::Bool {
        let adt_def = self.adt_defs.get(adt_name);
        let tag = if let Some(c) =
            adt_def.and_then(|d| d.constructors.iter().find(|c| c.name == ctor_name))
        {
            c.tag
        } else if let Some(def) = adt_def {
            let names: Vec<&str> = def.constructors.iter().map(|c| c.name.as_str()).collect();
            crate::encode_adt_policy::adt_ctor_tag_or_zero(&names, ctor_name)
        } else {
            0
        };

        let tag_fn_name = crate::encode_atom_policy::adt_tag_uf_name(adt_name);
        let tag_fn = self.make_func(&tag_fn_name, 1);
        let tag_val = tag_fn
            .apply(&[value as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        tag_val.eq(ast::Int::from_i64(tag))
    }

    /// Access a field of a constructed ADT value.
    ///
    /// Returns `accessor(x)` as a Z3 Int. UF name via
    /// [`crate::encode_atom_policy::adt_accessor_uf_name`] (parity with
    /// [`crate::encode_adt_policy::adt_accessor_smtlib`]).
    pub(crate) fn adt_accessor(
        &mut self,
        adt_name: &str,
        accessor: &str,
        value: &ast::Int,
    ) -> ast::Int {
        let acc_fn_name = crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
        let acc_fn = self.make_func(&acc_fn_name, 1);
        acc_fn
            .apply(&[value as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int())
    }
}
