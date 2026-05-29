//! L-GENERIC-NESTING-DEPTH (Info): a type expression nested deeper than the
//! configured threshold is a readability hint (《Linter 规则草案 §10》, 《泛型与
//! 类型构造器规范草案》).
//!
//! Depth is `helpers::type_expr_depth` (a leaf is 1, a constructor is
//! `1 + max(arg)`). The spec's "默认阈值 3 层" is read as *fire when depth
//! exceeds 3* (i.e. ≥ 4): a type like `Queue<thread<worker>>` (depth 3) stays
//! quiet, while `List<List<List<List<int>>>>` (depth 5) is flagged. This keeps
//! the project's own idiomatic types clean while catching genuinely deep nests.
//!
//! Every type-bearing site is visited (let annotations, params, return types,
//! `expose` / `define` fields, and `TypeCtor` type arguments) by walking the
//! AST directly; the depth is computed once per site root, so a single deep
//! type is reported once rather than at every nested layer.

use tessera_ast::*;
use tessera_types::TypeEnv;

use crate::{Diagnostic, LintPass};
use super::helpers::type_expr_depth;

/// Fire when nesting depth is strictly greater than this.
const MAX_DEPTH: usize = 3;

pub struct GenericNestingDepth;

impl LintPass for GenericNestingDepth {
    fn name(&self) -> &'static str { "L-GENERIC-NESTING-DEPTH" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut v = V { diags: vec![] };
        for item in &program.items {
            v.visit_item(item);
        }
        v.diags
    }
}

struct V {
    diags: Vec<Diagnostic>,
}

impl V {
    /// Check one *site-root* type expression and report once if too deep. Does
    /// not recurse into the type's own arguments (the depth already accounts for
    /// them), so a deep type is reported a single time.
    fn check_type(&mut self, te: &TypeExpr) {
        let depth = type_expr_depth(te);
        if depth > MAX_DEPTH {
            if let Some(span) = te.span() {
                self.diags.push(
                    Diagnostic::info(
                        "L-GENERIC-NESTING-DEPTH",
                        format!("type nesting depth {depth} exceeds {MAX_DEPTH}; consider a type alias or a flatter design"),
                        span,
                    ),
                );
            }
        }
    }

    fn visit_item(&mut self, item: &TopLevelItem) {
        match item {
            TopLevelItem::FuncDef(f) => self.visit_func(f),
            TopLevelItem::ScopeTemplateDecl(d) => {
                for p in &d.params { self.check_type(&p.ty); }
                for m in &d.members {
                    match m {
                        ScopeTemplateMember::OnEnter(f)
                        | ScopeTemplateMember::OnExit(f)
                        | ScopeTemplateMember::MemberFunc(f) => self.visit_func(f),
                        ScopeTemplateMember::Define(e) => self.visit_expose(e),
                    }
                }
            }
            TopLevelItem::ThreadTemplateDecl(d) => {
                for p in &d.params { self.check_type(&p.ty); }
                for m in &d.members {
                    match m {
                        ThreadTemplateMember::OnEnter(f)
                        | ThreadTemplateMember::OnExit(f)
                        | ThreadTemplateMember::OnTerminate(f)
                        | ThreadTemplateMember::MemberFunc(f) => self.visit_func(f),
                        ThreadTemplateMember::Handler(h) => {
                            for p in &h.params { self.check_type(&p.ty); }
                            self.check_type(&h.return_type);
                            self.visit_block(&h.body);
                        }
                        ThreadTemplateMember::Expose(e)
                        | ThreadTemplateMember::ExposeMutable(e)
                        | ThreadTemplateMember::Define(e) => self.visit_expose(e),
                    }
                }
            }
            TopLevelItem::Statement(s) => self.visit_stmt(s),
        }
    }

    fn visit_func(&mut self, f: &FuncDef) {
        for p in &f.params { self.check_type(&p.ty); }
        self.check_type(&f.return_type);
        self.visit_block(&f.body);
    }

    fn visit_expose(&mut self, e: &ExposeDecl) {
        self.check_type(&e.ty);
        if let Some(init) = &e.initializer { self.visit_expr(init); }
    }

    fn visit_block(&mut self, b: &Block) {
        for s in &b.stmts { self.visit_stmt(s); }
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(l) => {
                if let Some(ty) = &l.ty { self.check_type(ty); }
                self.visit_expr(&l.init);
            }
            Stmt::Assign(a) => self.visit_expr(&a.value),
            Stmt::If(i) => {
                self.visit_expr(&i.condition);
                self.visit_block(&i.then_block);
                match &i.else_branch {
                    Some(ElseBranch::Else(b)) => self.visit_block(b),
                    Some(ElseBranch::ElseIf(i2)) => self.visit_stmt(&Stmt::If((**i2).clone())),
                    None => {}
                }
            }
            Stmt::While(w) => { self.visit_expr(&w.condition); self.visit_block(&w.body); }
            Stmt::For(f) => {
                if let Some(init) = &f.init { self.visit_stmt(init); }
                if let Some(c) = &f.condition { self.visit_expr(c); }
                if let Some(u) = &f.update { self.visit_stmt(u); }
                self.visit_block(&f.body);
            }
            Stmt::Return(r) => { if let Some(v) = &r.value { self.visit_expr(v); } }
            Stmt::ThreadSpawn(ts) => {
                for a in &ts.args { self.visit_expr(a); }
                self.visit_block(&ts.body);
                if let ThreadTemplateRef::Anonymous(decl) = &ts.template {
                    self.visit_item(&TopLevelItem::ThreadTemplateDecl((**decl).clone()));
                }
            }
            Stmt::ScopeBlock(sb) => {
                for a in &sb.args { self.visit_expr(a); }
                self.visit_block(&sb.body);
                if let ScopeTemplateRef::Anonymous(decl) = &sb.template {
                    self.visit_item(&TopLevelItem::ScopeTemplateDecl((**decl).clone()));
                }
            }
            Stmt::ExclusiveBlock(eb) => self.visit_block(&eb.body),
            Stmt::Expr(es) => self.visit_expr(&es.expr),
            Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        if let Expr::TypeCtor(tc) = e {
            // Treat the constructed type as a site root: depth = 1 + max(arg).
            let depth = 1 + tc.type_args.iter().map(type_expr_depth).max().unwrap_or(0);
            if depth > MAX_DEPTH {
                self.diags.push(
                    Diagnostic::info(
                        "L-GENERIC-NESTING-DEPTH",
                        format!("type nesting depth {depth} exceeds {MAX_DEPTH}; consider a type alias or a flatter design"),
                        tc.span,
                    ),
                );
            }
        }
        // Recurse into sub-expressions for nested constructors / blocks.
        tessera_ast::visitor::walk_expr(&mut ExprWalker { v: self }, e);
    }
}

/// Thin adapter so we can reuse `walk_expr` for recursion while keeping our own
/// `visit_expr` (which the trait's `walk_expr` calls back into).
struct ExprWalker<'a> { v: &'a mut V }

impl tessera_ast::visitor::Visitor for ExprWalker<'_> {
    fn visit_expr(&mut self, e: &Expr) {
        self.v.visit_expr(e);
    }
}
