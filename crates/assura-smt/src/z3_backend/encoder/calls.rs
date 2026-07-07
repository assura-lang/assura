//! Function/method call, field access, and index encoding.

use crate::*;
use assura_ast::SpExpr;
use z3::ast;

use super::Encoder;
use super::value::Z3Value;

impl Encoder {
    /// Encode a function call as an uninterpreted function application.
    /// Known boolean methods return Bool; everything else returns Int.
    pub(crate) fn adt_for_constructor(&self, ctor_name: &str) -> Option<String> {
        self.adt_defs.iter().find_map(|(adt_name, def)| {
            def.constructors
                .iter()
                .any(|c| c.name == ctor_name)
                .then_some(adt_name.clone())
        })
    }

    /// Encode a function/method call to Z3 (integer-encoding mode).
    ///
    /// 1. [`crate::encode_call_policy::classify_encode_call_preamble`] (ADT ctor /
    ///    length fast paths before integer arg encoding).
    /// 2. [`crate::encode_call_policy::classify_encode_call`] for the main builtin /
    ///    UF order (min/max → bool UF → seq/string builtins → abs → get/set/put →
    ///    size UF → uninterpreted). Term construction stays here.
    pub(crate) fn encode_call(&mut self, func_name: &str, args: &[SpExpr]) -> Z3Value {
        use crate::encode_call_policy::{
            EncodeCallPreamble, classify_encode_call, classify_encode_call_preamble,
        };

        let is_upper = func_name.chars().next().is_some_and(|c| c.is_uppercase());
        match classify_encode_call_preamble(func_name, args.len(), is_upper) {
            EncodeCallPreamble::PossibleAdtConstructor => {
                self.init_adt_infrastructure();
                let arg_vals: Vec<ast::Int> = args
                    .iter()
                    .map(|a| self.encode_expr(a).as_int(&mut self.fresh_counter))
                    .collect();
                if let Some(adt_name) = self.adt_for_constructor(func_name) {
                    return Z3Value::Int(self.adt_constructor(&adt_name, func_name, &arg_vals));
                }
                // Not a registered ctor: fall through to normal integer-arg encode
                // with the already-computed arg_vals (avoid double encode).
                let call_kind = classify_encode_call(func_name, arg_vals.len());
                return self.encode_call_with_arg_vals(func_name, args, &arg_vals, call_kind);
            }
            EncodeCallPreamble::LengthMethodArity1 => {
                // Native string theory: length(str_val) uses Z3's str.len
                if self.use_string_theory {
                    let arg_val = self.encode_expr(&args[0]);
                    if let Z3Value::Str(s) = &arg_val {
                        let len = s.length();
                        let zero = ast::Int::from_i64(0);
                        self.background_axioms.push(len.ge(&zero));
                        return Z3Value::Int(len);
                    }
                }
                // Canonical length for simple identifiers (#267).
                if let Expr::Ident(name) = &args[0].node {
                    return Z3Value::Int(self.canonical_length(name));
                }
                // Non-ident receiver: fall through to SizeFieldUf / normal path.
            }
            EncodeCallPreamble::None => {}
        }

        let arg_vals: Vec<ast::Int> = args
            .iter()
            .map(|a| self.encode_expr(a).as_int(&mut self.fresh_counter))
            .collect();
        // Single classify pass (parity with CVC5 / encode_call_policy order); term
        // bodies stay in each arm. Guards use `call_kind` instead of repeating
        // `is_*_builtin` + `debug_assert_encode_call_kind` pairs.
        let call_kind = classify_encode_call(func_name, arg_vals.len());
        self.encode_call_with_arg_vals(func_name, args, &arg_vals, call_kind)
    }

    /// Main `encode_call` body after integer arg encoding (shared with ADT miss fallthrough).
    fn encode_call_with_arg_vals(
        &mut self,
        func_name: &str,
        args: &[SpExpr],
        arg_vals: &[ast::Int],
        call_kind: crate::encode_call_policy::EncodeCallKind,
    ) -> Z3Value {
        use crate::encode_call_policy::EncodeCallKind;

        match call_kind {
            EncodeCallKind::MinMax => self.encode_call_min_max(func_name, arg_vals),
            EncodeCallKind::BoolReturningUf => self.encode_call_bool_uf(func_name, args, arg_vals),
            EncodeCallKind::Substring => self.encode_call_substring(args, arg_vals),
            EncodeCallKind::ConcatAppend => self.encode_call_concat_append(args, arg_vals),
            EncodeCallKind::IndexOf => self.encode_call_index_of(args, arg_vals),
            EncodeCallKind::CharAt => self.encode_call_char_at(args, arg_vals),
            EncodeCallKind::Replace => self.encode_call_replace(),
            EncodeCallKind::Split => self.encode_call_split(),
            EncodeCallKind::TrimOrCaseFold => self.encode_call_trim_or_case_fold(args, arg_vals),
            EncodeCallKind::CloneOrReverse => self.encode_call_clone_or_reverse(args, arg_vals),
            EncodeCallKind::Clear => self.encode_call_clear(),
            EncodeCallKind::Push => self.encode_call_push(args, arg_vals),
            EncodeCallKind::PopOrTail => self.encode_call_pop_or_tail(args, arg_vals),
            EncodeCallKind::Insert => self.encode_call_insert(args, arg_vals),
            EncodeCallKind::Remove => self.encode_call_remove(args, arg_vals),
            EncodeCallKind::Slice => self.encode_call_slice(args, arg_vals),
            EncodeCallKind::Take => self.encode_call_take(args, arg_vals),
            EncodeCallKind::Drop => self.encode_call_drop(args, arg_vals),
            EncodeCallKind::First => self.encode_call_first(),
            EncodeCallKind::Abs => self.encode_call_abs(arg_vals),
            EncodeCallKind::Get => self.encode_call_get(arg_vals),
            EncodeCallKind::Set => self.encode_call_set(args, arg_vals),
            EncodeCallKind::Put => self.encode_call_put(args, arg_vals),
            EncodeCallKind::SizeFieldUf => {
                self.encode_call_size_field_uf(func_name, args, arg_vals)
            }
            EncodeCallKind::UninterpretedUf => self.encode_call_uninterpreted(func_name, arg_vals),
        }
    }

    // -----------------------------------------------------------------------
    // Per-kind encode_call handlers (extracted from encode_call_with_arg_vals)
    // -----------------------------------------------------------------------

    /// Encode `min`/`max` as Z3 ite (not unconstrained UF).
    fn encode_call_min_max(&mut self, func_name: &str, arg_vals: &[ast::Int]) -> Z3Value {
        let a = &arg_vals[0];
        let b = &arg_vals[1];
        let a_le_b = a.le(b);
        let result = if func_name == "min" {
            a_le_b.ite(a, b)
        } else {
            a_le_b.ite(b, a)
        };
        Z3Value::Int(result)
    }

    /// Encode a bool-returning UF with optional specialized axioms.
    fn encode_call_bool_uf(
        &mut self,
        func_name: &str,
        args: &[SpExpr],
        arg_vals: &[ast::Int],
    ) -> Z3Value {
        use crate::encode_call_policy::{BoolCallAxiom, classify_bool_call_axiom};

        let bool_sort = z3::Sort::bool();
        let int_sort = z3::Sort::int();
        let param_sorts: Vec<&z3::Sort> = (0..arg_vals.len()).map(|_| &int_sort).collect();
        let decl = z3::FuncDecl::new(func_name, &param_sorts, &bool_sort);
        let arg_refs: Vec<&dyn z3::ast::Ast> =
            arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
        let result = decl.apply(&arg_refs);
        let b = result.as_bool().unwrap_or_else(|| self.fresh_bool());
        match classify_bool_call_axiom(func_name, arg_vals.len()) {
            BoolCallAxiom::IsEmpty => {
                // is_empty(x) <=> len(x) == 0 (bidirectional; sound).
                let coll = &arg_vals[0];
                let coll_expr = &args[0].node;
                let len_val = self.collection_len_of(coll_expr, coll, "len");
                let zero = ast::Int::from_i64(0);
                let len_is_zero = len_val.eq(&zero);
                self.background_axioms.push(b.implies(&len_is_zero));
                self.background_axioms.push(len_is_zero.implies(&b));
            }
            BoolCallAxiom::Contains => {
                // contains(s, sub) => len(s) >= len(sub) (contiguous substring; sound).
                let hay_expr = &args[0].node;
                let needle_expr = &args[1].node;
                let hay_len = self.collection_len_of(hay_expr, &arg_vals[0], "len");
                let needle_len = self.collection_len_of(needle_expr, &arg_vals[1], "len");
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(hay_len.ge(&zero));
                self.background_axioms.push(needle_len.ge(&zero));
                let hay_ge_needle = hay_len.ge(&needle_len);
                self.background_axioms.push(b.implies(&hay_ge_needle));
            }
            BoolCallAxiom::AffixPredicate => {
                // starts_with/ends_with(s, affix) => len(s) >= len(affix) (sound).
                let s_expr = &args[0].node;
                let aff_expr = &args[1].node;
                let s_len = self.collection_len_of(s_expr, &arg_vals[0], "len");
                let aff_len = self.collection_len_of(aff_expr, &arg_vals[1], "len");
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(s_len.ge(&zero));
                self.background_axioms.push(aff_len.ge(&zero));
                let s_ge_aff = s_len.ge(&aff_len);
                self.background_axioms.push(b.implies(&s_ge_aff));
                // Empty affix: starts_with/ends_with always hold (prefix/suffix of length 0).
                let aff_is_zero = aff_len.eq(&zero);
                self.background_axioms.push(aff_is_zero.implies(&b));
            }
            BoolCallAxiom::ContainsKey => {
                // contains_key(m, k) => size(m) >= 1 (key present implies non-empty map; sound).
                let map_expr = &args[0].node;
                let map_size = self.collection_len_of(map_expr, &arg_vals[0], "size");
                // Also link size <-> len for maps (size method vs len).
                let map_len = self.collection_len_of(map_expr, &arg_vals[0], "len");
                self.background_axioms.push(map_size.eq(&map_len));
                let one = ast::Int::from_i64(1);
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(map_size.ge(&zero));
                let size_ge_one = map_size.ge(&one);
                self.background_axioms.push(b.implies(&size_ge_one));
            }
            BoolCallAxiom::Generic => {
                // No specialized axioms; generic bool UF.
            }
        }
        Z3Value::Bool(b)
    }

    /// Encode `substring(str, start, end)` with length and bounds axioms.
    fn encode_call_substring(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let str_expr = &args[0].node;
        let str_val = &arg_vals[0];
        let start = &arg_vals[1];
        let end = &arg_vals[2];
        let result = self.fresh_int();
        let zero = ast::Int::from_i64(0);
        self.background_axioms.push(start.ge(&zero));
        self.background_axioms.push(start.le(end));
        let str_len = self.collection_len_of(
            str_expr,
            str_val,
            crate::encode_atom_policy::FIELD_LEN_UF_NAME,
        );
        self.background_axioms.push(end.le(&str_len));
        let diff = ast::Int::sub(&[end, start]);
        self.assert_collection_len_eq(&result, &diff, crate::encode_atom_policy::FIELD_LEN_UF_NAME);
        Z3Value::Int(result)
    }

    /// Encode `concat`/`append` with length-sum axiom.
    fn encode_call_concat_append(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let l_expr = &args[0].node;
        let r_expr = &args[1].node;
        let l = &arg_vals[0];
        let r = &arg_vals[1];
        let result = self.fresh_int();
        let len_l = self.collection_len_of(l_expr, l, crate::encode_atom_policy::FIELD_LEN_UF_NAME);
        let len_r = self.collection_len_of(r_expr, r, crate::encode_atom_policy::FIELD_LEN_UF_NAME);
        let zero = ast::Int::from_i64(0);
        self.background_axioms.push(len_l.ge(&zero));
        self.background_axioms.push(len_r.ge(&zero));
        let sum = ast::Int::add(&[&len_l, &len_r]);
        self.assert_collection_len_eq(&result, &sum, crate::encode_atom_policy::FIELD_LEN_UF_NAME);
        // Also result length >= each operand (redundant but helps some goals).
        self.background_axioms.push(sum.ge(&len_l));
        self.background_axioms.push(sum.ge(&len_r));
        Z3Value::Int(result)
    }

    /// Encode `index_of`/`find` with range axiom (-1 <= result < len).
    fn encode_call_index_of(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let str_expr = &args[0].node;
        let str_val = &arg_vals[0];
        let result = self.fresh_int();
        let neg_one = ast::Int::from_i64(-1);
        self.background_axioms.push(result.ge(&neg_one));
        let str_len = self.collection_len_of(
            str_expr,
            str_val,
            crate::encode_atom_policy::FIELD_LEN_UF_NAME,
        );
        // result < len(str) covers both found indices and -1 when len >= 0.
        self.background_axioms.push(result.lt(&str_len));
        Z3Value::Int(result)
    }

    /// Encode `char_at(str, idx)` with index bounds axioms.
    fn encode_call_char_at(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let str_expr = &args[0].node;
        let str_val = &arg_vals[0];
        let idx = &arg_vals[1];
        let zero = ast::Int::from_i64(0);
        self.background_axioms.push(idx.ge(&zero));
        let str_len = self.collection_len_of(
            str_expr,
            str_val,
            crate::encode_atom_policy::FIELD_LEN_UF_NAME,
        );
        self.background_axioms.push(idx.lt(&str_len));
        Z3Value::Int(self.fresh_int())
    }

    /// Encode `replace(str, old, new)` with weak length axiom (>= 0).
    fn encode_call_replace(&mut self) -> Z3Value {
        let result = self.fresh_int();
        let res_len = self.fresh_int();
        let zero = ast::Int::from_i64(0);
        self.background_axioms.push(res_len.ge(&zero));
        self.assert_collection_len_eq(
            &result,
            &res_len,
            crate::encode_atom_policy::FIELD_LEN_UF_NAME,
        );
        Z3Value::Int(result)
    }

    /// Encode `split(str, delim)` with collection length >= 1.
    fn encode_call_split(&mut self) -> Z3Value {
        let result = self.fresh_int();
        let one = ast::Int::from_i64(1);
        let len_decl = self.make_func(crate::encode_atom_policy::LEN_UF_NAME, 1);
        let res_len = len_decl
            .apply(&[&result as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        self.background_axioms.push(res_len.ge(&one));
        Z3Value::Int(result)
    }

    /// Encode `trim`/`to_lower`/`to_upper` with result length <= input length.
    fn encode_call_trim_or_case_fold(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let str_expr = &args[0].node;
        let str_val = &arg_vals[0];
        let result = self.fresh_int();
        let str_len = self.collection_len_of(
            str_expr,
            str_val,
            crate::encode_atom_policy::FIELD_LEN_UF_NAME,
        );
        let len_decl = self.make_func(crate::encode_atom_policy::FIELD_LEN_UF_NAME, 1);
        let res_len = len_decl
            .apply(&[&result as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        let zero = ast::Int::from_i64(0);
        self.background_axioms.push(res_len.ge(&zero));
        self.background_axioms.push(res_len.le(&str_len));
        Z3Value::Int(result)
    }

    /// Encode length-preserving `clone`/`to_string`/`reverse`.
    fn encode_call_clone_or_reverse(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let src_expr = &args[0].node;
        let src = &arg_vals[0];
        let result = self.fresh_int();
        let old_len = self.collection_len_of(src_expr, src, "len");
        self.assert_collection_len_eq(&result, &old_len, "len");
        Z3Value::Int(result)
    }

    /// Encode `clear(seq)` with length == 0.
    fn encode_call_clear(&mut self) -> Z3Value {
        let result = self.fresh_int();
        let zero = ast::Int::from_i64(0);
        self.assert_collection_len_eq(&result, &zero, "len");
        Z3Value::Int(result)
    }

    /// Encode `push`/`push_back`/`push_front` with length = old + 1.
    fn encode_call_push(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let src_expr = &args[0].node;
        let src = &arg_vals[0];
        let result = self.fresh_int();
        let one = ast::Int::from_i64(1);
        let old_len = self.collection_len_of(src_expr, src, "len");
        let new_len = ast::Int::add(&[&old_len, &one]);
        self.assert_collection_len_eq(&result, &new_len, "len");
        Z3Value::Int(result)
    }

    /// Encode `pop`/`tail`/`rest` with length = max(0, old - 1).
    fn encode_call_pop_or_tail(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let src_expr = &args[0].node;
        let src = &arg_vals[0];
        let result = self.fresh_int();
        let zero = ast::Int::from_i64(0);
        let one = ast::Int::from_i64(1);
        let old_len = self.collection_len_of(src_expr, src, "len");
        let dec = ast::Int::sub(&[&old_len, &one]);
        let new_len = old_len.ge(&one).ite(&dec, &zero);
        self.assert_collection_len_eq(&result, &new_len, "len");
        Z3Value::Int(result)
    }

    /// Encode `insert(seq, idx, val)` with length + 1 and store axiom.
    fn encode_call_insert(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let src_expr = &args[0].node;
        let src = &arg_vals[0];
        let idx = &arg_vals[1];
        let val = &arg_vals[2];
        let result = self.fresh_int();
        let one = ast::Int::from_i64(1);
        let zero = ast::Int::from_i64(0);
        let old_len = self.collection_len_of(src_expr, src, "len");
        let new_len = ast::Int::add(&[&old_len, &one]);
        self.assert_collection_len_eq(&result, &new_len, "len");
        self.background_axioms.push(idx.ge(&zero));
        self.background_axioms.push(idx.le(&old_len));
        let get_decl = self.make_func(crate::encode_atom_policy::INDEX_UF_NAME, 2);
        let at_idx = get_decl
            .apply(&[&result as &dyn z3::ast::Ast, idx as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        self.background_axioms.push(at_idx.eq(val));
        Z3Value::Int(result)
    }

    /// Encode `remove`/`remove_at` with length = max(0, old - 1).
    fn encode_call_remove(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let src_expr = &args[0].node;
        let src = &arg_vals[0];
        let result = self.fresh_int();
        let zero = ast::Int::from_i64(0);
        let one = ast::Int::from_i64(1);
        let old_len = self.collection_len_of(src_expr, src, "len");
        let dec = ast::Int::sub(&[&old_len, &one]);
        let new_len = old_len.ge(&one).ite(&dec, &zero);
        self.assert_collection_len_eq(&result, &new_len, "len");
        Z3Value::Int(result)
    }

    /// Encode `slice(seq, start, end)` with length and bounds axioms.
    fn encode_call_slice(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let src_expr = &args[0].node;
        let src = &arg_vals[0];
        let start = &arg_vals[1];
        let end = &arg_vals[2];
        let result = self.fresh_int();
        let zero = ast::Int::from_i64(0);
        let old_len = self.collection_len_of(src_expr, src, "len");
        self.background_axioms.push(start.ge(&zero));
        self.background_axioms.push(start.le(end));
        self.background_axioms.push(end.le(&old_len));
        let diff = ast::Int::sub(&[end, start]);
        self.assert_collection_len_eq(&result, &diff, "len");
        Z3Value::Int(result)
    }

    /// Encode `take(seq, n)` with length = min(n, old_len).
    fn encode_call_take(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let src_expr = &args[0].node;
        let src = &arg_vals[0];
        let n = &arg_vals[1];
        let result = self.fresh_int();
        let zero = ast::Int::from_i64(0);
        let old_len = self.collection_len_of(src_expr, src, "len");
        self.background_axioms.push(n.ge(&zero));
        let taken = n.le(&old_len).ite(n, &old_len);
        self.assert_collection_len_eq(&result, &taken, "len");
        Z3Value::Int(result)
    }

    /// Encode `drop(seq, n)` with length = max(0, old_len - n).
    fn encode_call_drop(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let src_expr = &args[0].node;
        let src = &arg_vals[0];
        let n = &arg_vals[1];
        let result = self.fresh_int();
        let zero = ast::Int::from_i64(0);
        let old_len = self.collection_len_of(src_expr, src, "len");
        self.background_axioms.push(n.ge(&zero));
        let rem = ast::Int::sub(&[&old_len, n]);
        let dropped = n.le(&old_len).ite(&rem, &zero);
        self.assert_collection_len_eq(&result, &dropped, "len");
        Z3Value::Int(result)
    }

    /// Encode `first`/`last`/`head` as a fresh unconstrained int.
    fn encode_call_first(&mut self) -> Z3Value {
        Z3Value::Int(self.fresh_int())
    }

    /// Encode `abs(x)` as `if x >= 0 then x else -x`.
    fn encode_call_abs(&mut self, arg_vals: &[ast::Int]) -> Z3Value {
        let x = &arg_vals[0];
        let zero = ast::Int::from_i64(0);
        let neg_x = x.unary_minus();
        let cond = x.ge(&zero);
        Z3Value::Int(cond.ite(x, &neg_x))
    }

    /// Encode `get(coll, key)` as UF unified with `__index`.
    fn encode_call_get(&mut self, arg_vals: &[ast::Int]) -> Z3Value {
        let coll = &arg_vals[0];
        let key = &arg_vals[1];
        let get_decl = self.make_func(crate::encode_atom_policy::GET_UF_NAME, 2);
        let via_get = get_decl
            .apply(&[coll as &dyn z3::ast::Ast, key as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        let idx_decl = self.make_func(crate::encode_atom_policy::INDEX_UF_NAME, 2);
        let via_idx = idx_decl
            .apply(&[coll as &dyn z3::ast::Ast, key as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        self.background_axioms.push(via_get.eq(&via_idx));
        Z3Value::Int(via_get)
    }

    /// Encode `set(arr, index, value)` with store axiom and length preservation.
    fn encode_call_set(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let arr_expr = &args[0].node;
        let arr = &arg_vals[0];
        let idx = &arg_vals[1];
        let val = &arg_vals[2];
        let result = self.fresh_int();
        let zero = ast::Int::from_i64(0);
        // Weak index non-negativity (callers often require i >= 0 separately).
        self.background_axioms.push(idx.ge(&zero));
        // Read-over-write via both get and __index (keep aliases aligned).
        let get_decl = self.make_func(crate::encode_atom_policy::GET_UF_NAME, 2);
        let get_at_idx = get_decl
            .apply(&[&result as &dyn z3::ast::Ast, idx as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        self.background_axioms.push(get_at_idx.eq(val));
        let idx_decl = self.make_func(crate::encode_atom_policy::INDEX_UF_NAME, 2);
        let via_idx = idx_decl
            .apply(&[&result as &dyn z3::ast::Ast, idx as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        self.background_axioms.push(via_idx.eq(val));
        // len(result) == len(original); use canonical length for named arrays (#267).
        let old_len = self.collection_len_of(arr_expr, arr, "len");
        self.assert_collection_len_eq(&result, &old_len, "len");
        Z3Value::Int(result)
    }

    /// Encode `put(map, key, value)` with read-over-write and size axioms.
    fn encode_call_put(&mut self, args: &[SpExpr], arg_vals: &[ast::Int]) -> Z3Value {
        let map_expr = &args[0].node;
        let map_val = &arg_vals[0];
        let key = &arg_vals[1];
        let value = &arg_vals[2];
        let new_map = self.fresh_int();
        // Read-over-write axiom: get(put(m, k, v), k) == v
        let get_decl = self.make_func(crate::encode_atom_policy::GET_UF_NAME, 2);
        let get_result = get_decl
            .apply(&[&new_map as &dyn z3::ast::Ast, key as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        self.background_axioms.push(get_result.eq(value));
        // contains_key(put(m, k, v), k) always holds (write implies key present).
        let bool_sort = z3::Sort::bool();
        let int_sort = z3::Sort::int();
        let ck_decl = z3::FuncDecl::new("contains_key", &[&int_sort, &int_sort], &bool_sort);
        let ck = ck_decl
            .apply(&[&new_map as &dyn z3::ast::Ast, key as &dyn z3::ast::Ast])
            .as_bool()
            .unwrap_or_else(|| self.fresh_bool());
        self.background_axioms.push(ck);
        // size(new_map) >= size(map); link size <-> len on both maps.
        let old_size = self.collection_len_of(map_expr, map_val, "size");
        let old_len = self.collection_len_of(map_expr, map_val, "len");
        self.background_axioms.push(old_size.eq(&old_len));
        let size_decl = self.make_func(crate::encode_atom_policy::SIZE_UF_NAME, 1);
        let new_size = size_decl
            .apply(&[&new_map as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        let zero = ast::Int::from_i64(0);
        let one = ast::Int::from_i64(1);
        self.background_axioms.push(new_size.ge(&old_size));
        self.background_axioms.push(new_size.ge(&zero));
        // Key present => size at least 1.
        self.background_axioms.push(new_size.ge(&one));
        let len_decl = self.make_func(crate::encode_atom_policy::LEN_UF_NAME, 1);
        let new_len = len_decl
            .apply(&[&new_map as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        self.background_axioms.push(new_len.eq(&new_size));
        Z3Value::Int(new_map)
    }

    /// Encode size/length method with non-negativity axiom.
    fn encode_call_size_field_uf(
        &mut self,
        func_name: &str,
        args: &[SpExpr],
        arg_vals: &[ast::Int],
    ) -> Z3Value {
        if arg_vals.len() == 1 {
            let coll_expr = &args[0].node;
            let coll = &arg_vals[0];
            // Named collections: always use the canonical length variable.
            if let Expr::Ident(name) = coll_expr {
                return Z3Value::Int(self.canonical_length(name));
            }
            let len_val = self.collection_len_of(coll_expr, coll, "len");
            let zero = ast::Int::from_i64(0);
            self.background_axioms.push(len_val.ge(&zero));
            // Link the requested method UF to the same length value.
            if func_name != "len" {
                let decl = self.make_func(func_name, 1);
                let via_method = decl
                    .apply(&[coll as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                self.background_axioms.push(via_method.eq(&len_val));
            }
            // Keep __field_len aligned (string/method `.length()` on temporaries).
            let fl = self.make_func(crate::encode_atom_policy::FIELD_LEN_UF_NAME, 1);
            let via_fl = fl
                .apply(&[coll as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(via_fl.eq(&len_val));
            return Z3Value::Int(len_val);
        }
        // Multi-arity fallback: generic non-negative UF.
        let decl = self.make_func(func_name, arg_vals.len());
        let arg_refs: Vec<&dyn z3::ast::Ast> =
            arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
        let result = decl.apply(&arg_refs);
        let len_val = result.as_int().unwrap_or_else(|| self.fresh_int());
        let zero = ast::Int::from_i64(0);
        self.background_axioms.push(len_val.ge(&zero));
        Z3Value::Int(len_val)
    }

    /// Encode a generic uninterpreted function application.
    fn encode_call_uninterpreted(&mut self, func_name: &str, arg_vals: &[ast::Int]) -> Z3Value {
        // Prefer same-file pure functional ensures over free UF when available.
        if let Some(expanded) = self.try_encode_call_via_callee_spec(func_name, arg_vals) {
            return expanded;
        }
        let decl = self.make_func(func_name, arg_vals.len());
        let arg_refs: Vec<&dyn z3::ast::Ast> =
            arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
        let result = decl.apply(&arg_refs);
        Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
    }

    /// Expand `f(args)` using a same-file `ensures { result == <expr> }` body.
    ///
    /// Binds formal parameter names to the already-encoded argument values,
    /// encodes the body, then restores prior bindings.
    fn try_encode_call_via_callee_spec(
        &mut self,
        func_name: &str,
        arg_vals: &[ast::Int],
    ) -> Option<Z3Value> {
        let spec = self.callee_specs.get(func_name)?.clone();
        if spec.param_names.len() != arg_vals.len() {
            return None;
        }
        let mut saved: Vec<(String, Option<Z3Value>)> = Vec::new();
        for (param, arg) in spec.param_names.iter().zip(arg_vals.iter()) {
            saved.push((param.clone(), self.vars.remove(param)));
            self.vars.insert(param.clone(), Z3Value::Int(arg.clone()));
        }
        let body = spec.result_body.clone();
        let encoded = self.encode_expr(&body);
        for (param, old) in saved {
            match old {
                Some(v) => {
                    self.vars.insert(param, v);
                }
                None => {
                    self.vars.remove(&param);
                }
            }
        }
        Some(encoded)
    }

    /// Encode field access as uninterpreted function: field_name(object).
    /// Known boolean fields return Bool; size fields return non-negative Int.
    ///
    /// Note (#191): When the object is `result` (a free Z3 variable), the
    /// field access `__field_len(result)` is also unconstrained. This means
    /// ensures clauses like `result.length() <= raw.length()` produce
    /// spurious counterexamples because Z3 can assign any value to
    /// `__field_len(result)`. This is a known limitation; see the doc
    /// comment on `verify_clauses_with_types` for details.
    pub(crate) fn encode_field_access(&mut self, obj: &SpExpr, field: &str) -> Z3Value {
        // #198 / #267: plan via encode_field_policy (canonical length, flatten, shallow).
        use crate::encode_field_policy::{FieldValueKind, classify_field_value_kind};
        match crate::encode_field_policy::plan_field_access(obj, field) {
            crate::encode_field_policy::FieldAccessPlan::CanonicalLength { obj_name } => {
                return Z3Value::Int(self.canonical_length(&obj_name));
            }
            crate::encode_field_policy::FieldAccessPlan::Flatten(flat_name) => {
                return match classify_field_value_kind(field) {
                    FieldValueKind::Bool => {
                        let v = ast::Bool::new_const(flat_name.as_str());
                        Z3Value::Bool(v)
                    }
                    FieldValueKind::SizeNonNeg => {
                        let v = self.get_or_create_int(&flat_name);
                        let zero = ast::Int::from_i64(0);
                        self.background_axioms.push(v.ge(&zero));
                        Z3Value::Int(v)
                    }
                    FieldValueKind::Int => {
                        let v = self.get_or_create_int(&flat_name);
                        Z3Value::Int(v)
                    }
                };
            }
            crate::encode_field_policy::FieldAccessPlan::ShallowUf { .. } => {}
        }

        // Native string theory: .length() on a non-ident Str value uses Z3's str.len
        // (ident length is CanonicalLength above; policy marks may_use_string_theory).
        if self.use_string_theory
            && crate::encode_field_policy::field_access_may_use_string_theory_length(obj, field)
        {
            let obj_val = self.encode_expr(obj);
            if let Z3Value::Str(s) = &obj_val {
                let len = s.length();
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(len.ge(&zero));
                return Z3Value::Int(len);
            }
            // Not a Str value; fall through to shallow UF
        }

        let obj_val = self.encode_expr(obj).as_int(&mut self.fresh_counter);
        let func_name = crate::encode_field_policy::field_uf_smtlib_name(field);
        match classify_field_value_kind(field) {
            FieldValueKind::Bool => {
                let bool_sort = z3::Sort::bool();
                let int_sort = z3::Sort::int();
                let decl = z3::FuncDecl::new(func_name.as_str(), &[&int_sort], &bool_sort);
                let result = decl.apply(&[&obj_val as &dyn z3::ast::Ast]);
                Z3Value::Bool(result.as_bool().unwrap_or_else(|| self.fresh_bool()))
            }
            FieldValueKind::SizeNonNeg => {
                let decl = self.make_func(&func_name, 1);
                let result = decl.apply(&[&obj_val as &dyn z3::ast::Ast]);
                let len_val = result.as_int().unwrap_or_else(|| self.fresh_int());
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(len_val.ge(&zero));
                Z3Value::Int(len_val)
            }
            FieldValueKind::Int => {
                let decl = self.make_func(&func_name, 1);
                let result = decl.apply(&[&obj_val as &dyn z3::ast::Ast]);
                Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
            }
        }
    }
}
