use crate::*;

pub trait Visitor: Sized {
    fn visit_program(&mut self, p: &Program) { walk_program(self, p); }
    fn visit_scope_template_decl(&mut self, d: &ScopeTemplateDecl) { walk_scope_template_decl(self, d); }
    fn visit_thread_template_decl(&mut self, d: &ThreadTemplateDecl) { walk_thread_template_decl(self, d); }
    fn visit_func_def(&mut self, f: &FuncDef) { walk_func_def(self, f); }
    fn visit_handler_def(&mut self, h: &HandlerDef) { walk_handler_def(self, h); }
    fn visit_expose_decl(&mut self, _e: &ExposeDecl, _mutable: bool) {}
    fn visit_block(&mut self, b: &Block) { walk_block(self, b); }
    fn visit_stmt(&mut self, s: &Stmt) { walk_stmt(self, s); }
    fn visit_expr(&mut self, e: &Expr) { walk_expr(self, e); }
    fn visit_type_expr(&mut self, _t: &TypeExpr) {}
}

pub fn walk_program<V: Visitor>(v: &mut V, p: &Program) {
    for item in &p.items {
        match item {
            TopLevelItem::ScopeTemplateDecl(d) => v.visit_scope_template_decl(d),
            TopLevelItem::ThreadTemplateDecl(d) => v.visit_thread_template_decl(d),
            TopLevelItem::FuncDef(_) => {}
            TopLevelItem::Statement(s) => v.visit_stmt(s),
        }
    }
}

pub fn walk_scope_template_decl<V: Visitor>(v: &mut V, d: &ScopeTemplateDecl) {
    for m in &d.members {
        match m {
            ScopeTemplateMember::OnEnter(f) | ScopeTemplateMember::OnExit(f)
            | ScopeTemplateMember::MemberFunc(f) => v.visit_func_def(f),
            ScopeTemplateMember::Define(e) => v.visit_expose_decl(e, false),
        }
    }
}

pub fn walk_thread_template_decl<V: Visitor>(v: &mut V, d: &ThreadTemplateDecl) {
    for m in &d.members {
        match m {
            ThreadTemplateMember::OnEnter(f) | ThreadTemplateMember::OnExit(f)
            | ThreadTemplateMember::OnTerminate(f) | ThreadTemplateMember::MemberFunc(f) => {
                v.visit_func_def(f);
            }
            ThreadTemplateMember::Handler(h) => v.visit_handler_def(h),
            ThreadTemplateMember::Expose(e) => v.visit_expose_decl(e, false),
            ThreadTemplateMember::ExposeMutable(e) => v.visit_expose_decl(e, true),
            ThreadTemplateMember::Define(e) => v.visit_expose_decl(e, false),
        }
    }
}

pub fn walk_func_def<V: Visitor>(v: &mut V, f: &FuncDef) {
    v.visit_block(&f.body);
}

pub fn walk_handler_def<V: Visitor>(v: &mut V, h: &HandlerDef) {
    v.visit_block(&h.body);
}

pub fn walk_block<V: Visitor>(v: &mut V, b: &Block) {
    for s in &b.stmts {
        v.visit_stmt(s);
    }
}

pub fn walk_stmt<V: Visitor>(v: &mut V, s: &Stmt) {
    match s {
        Stmt::Let(l) => v.visit_expr(&l.init),
        Stmt::Assign(a) => v.visit_expr(&a.value),
        Stmt::If(i) => {
            v.visit_expr(&i.condition);
            v.visit_block(&i.then_block);
            if let Some(eb) = &i.else_branch {
                match eb {
                    ElseBranch::Else(b) => v.visit_block(b),
                    ElseBranch::ElseIf(i2) => v.visit_stmt(&Stmt::If(*i2.clone())),
                }
            }
        }
        Stmt::While(w) => { v.visit_expr(&w.condition); v.visit_block(&w.body); }
        Stmt::For(f) => {
            if let Some(init) = &f.init { v.visit_stmt(init); }
            if let Some(cond) = &f.condition { v.visit_expr(cond); }
            if let Some(upd) = &f.update { v.visit_stmt(upd); }
            v.visit_block(&f.body);
        }
        Stmt::Return(r) => { if let Some(val) = &r.value { v.visit_expr(val); } }
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ThreadSpawn(ts) => {
            for arg in &ts.args { v.visit_expr(arg); }
            v.visit_block(&ts.body);
            if let ThreadTemplateRef::Anonymous(decl) = &ts.template {
                v.visit_thread_template_decl(decl);
            }
        }
        Stmt::ScopeBlock(sb) => {
            for arg in &sb.args { v.visit_expr(arg); }
            v.visit_block(&sb.body);
            if let ScopeTemplateRef::Anonymous(decl) = &sb.template {
                v.visit_scope_template_decl(decl);
            }
        }
        Stmt::ExclusiveBlock(eb) => v.visit_block(&eb.body),
        Stmt::Expr(es) => v.visit_expr(&es.expr),
    }
}

pub fn walk_expr<V: Visitor>(v: &mut V, e: &Expr) {
    match e {
        Expr::Lit(_) | Expr::Ident(_) => {}
        Expr::BinOp(b) => { v.visit_expr(&b.left); v.visit_expr(&b.right); }
        Expr::UnaryOp(u) => v.visit_expr(&u.operand),
        Expr::Call(c) => { v.visit_expr(&c.callee); for a in &c.args { v.visit_expr(a); } }
        Expr::MethodCall(m) => { v.visit_expr(&m.receiver); for a in &m.args { v.visit_expr(a); } }
        Expr::FieldAccess(f) => v.visit_expr(&f.object),
        Expr::Index(i) => { v.visit_expr(&i.object); v.visit_expr(&i.index); }
        Expr::Await(a) => v.visit_expr(&a.expr),
        Expr::Panic(p) => v.visit_expr(&p.message),
        Expr::Assert(a) => {
            v.visit_expr(&a.condition);
            if let Some(msg) = &a.message { v.visit_expr(msg); }
        }
        Expr::TypeCtor(tc) => { for a in &tc.args { v.visit_expr(a); } }
    }
}
