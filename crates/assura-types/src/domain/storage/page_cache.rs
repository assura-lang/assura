//! T087: STOR.2 Page cache contracts.

use assura_parser::ast::{ClauseKind, Expr, SpExpr};

use crate::TypeError;
use crate::checkers::*;
use crate::types::*;

#[derive(Debug, Clone)]
pub(crate) struct PageCacheChecker {
    pages: std::collections::HashMap<u64, PageInfo>,
    capacity: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PageInfo {
    pub dirty: bool,
    pub pinned: bool,
    pub pin_count: u32,
}

impl PageCacheChecker {
    pub fn new(capacity: usize) -> Self {
        Self {
            pages: std::collections::HashMap::new(),
            capacity,
        }
    }

    pub fn load_page(&mut self, page_id: u64) {
        self.pages.insert(
            page_id,
            PageInfo {
                dirty: false,
                pinned: false,
                pin_count: 0,
            },
        );
    }

    pub fn pin(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            p.pinned = true;
            p.pin_count += 1;
        }
    }

    pub fn unpin(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            if p.pin_count > 0 {
                p.pin_count -= 1;
            }
            if p.pin_count == 0 {
                p.pinned = false;
            }
        }
    }

    pub fn mark_dirty(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            p.dirty = true;
        }
    }

    pub fn flush(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            p.dirty = false;
        }
    }

    pub fn evict(&mut self, page_id: u64) -> Option<TypeError> {
        if let Some(p) = self.pages.get(&page_id) {
            if p.pinned {
                return Some(TypeError {
                    code: "A34001".into(),
                    message: format!("cannot evict pinned page {page_id}"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
            if p.dirty {
                return Some(TypeError {
                    code: "A34002".into(),
                    message: format!("evicting dirty page {page_id} without flush"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        self.pages.remove(&page_id);
        None
    }

    pub fn check_capacity(&self) -> Vec<TypeError> {
        if self.pages.len() > self.capacity {
            vec![TypeError {
                code: "A34003".into(),
                message: format!(
                    "page cache size {} exceeds capacity {}",
                    self.pages.len(),
                    self.capacity
                ),
                span: 0..1,
                secondary: None,
                suggestion: None,
            }]
        } else {
            vec![]
        }
    }
}

impl PageCacheChecker {
    /// Scan an expression for page cache operations.
    fn scan_expr(expr: &SpExpr, checker: &mut PageCacheChecker) {
        if let Some((name, args)) = extract_call(expr) {
            let page_id = args
                .first()
                .and_then(extract_int_literal)
                .unwrap_or(DEFAULT_PARAM_ZERO) as u64;
            match name {
                "load_page" | "load" | "fetch_page" => checker.load_page(page_id),
                "pin" | "pin_page" => checker.pin(page_id),
                "unpin" | "unpin_page" => checker.unpin(page_id),
                "mark_dirty" | "dirty" => checker.mark_dirty(page_id),
                "flush" | "flush_page" => checker.flush(page_id),
                "evict" | "evict_page" => {
                    checker.evict(page_id);
                }
                _ => {}
            }
        }
    }

    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker: Option<PageCacheChecker> = None;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "page_cache" || k == "buffer_pool" || k == "cache_policy")
                {
                    let capacity = match &clause.body.node {
                        Expr::Call { args, .. } => {
                            args.first()
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_PAGE_SIZE) as usize
                        }
                        Expr::Literal(assura_parser::ast::Literal::Int(s)) => {
                            s.parse::<usize>().unwrap_or(DEFAULT_PAGE_SIZE as usize)
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            kvs.iter()
                                .find(|(k, _)| {
                                    *k == "capacity" || *k == "size" || *k == "max_pages"
                                })
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PAGE_SIZE) as usize
                        }
                    };
                    if checker.is_none() {
                        checker = Some(PageCacheChecker::new(capacity));
                    }
                }
                if let Some(ch) = checker.as_mut()
                    && matches!(
                        clause.kind,
                        ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Other(_)
                    )
                {
                    Self::scan_expr(&clause.body, ch);
                }
            }
        }
        match checker {
            Some(ch) => ch.check_capacity(),
            None => Vec::new(),
        }
    }
}

impl Default for PageCacheChecker {
    fn default() -> Self {
        Self::new(1024)
    }
}
