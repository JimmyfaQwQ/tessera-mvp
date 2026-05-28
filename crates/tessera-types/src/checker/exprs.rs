//! Expression type checking. The bulk of the checker lives here because every
//! statement form eventually evaluates expressions; keeping the dispatch in
//! one file makes the method-on-receiver table easy to scan.

use tessera_ast::*;
use crate::{Type, FuncContext};

use super::TypeChecker;

impl<'e> TypeChecker<'e> {
    pub fn check_expr(&mut self, e: &Expr) -> Type {
        match e {
            Expr::Lit(l) => self.check_literal(l),
            Expr::Ident(i) => {
                self.env.lookup(&i.name).cloned().unwrap_or_else(|| {
                    self.env.error(format!("undefined variable '{}'", i.name), i.span);
                    Type::Error
                })
            }
            Expr::BinOp(b) => self.check_binop(b),
            Expr::UnaryOp(u) => self.check_unary(u),
            Expr::Call(c) => {
                let arg_types: Vec<Type> = c.args.iter().map(|a| self.check_expr(a)).collect();
                if let Expr::Ident(i) = &c.callee {
                    match i.name.as_str() {
                        "keepalive" => return Type::Never,
                        "getchar"   => return Type::Option(Box::new(Type::Char)),
                        "print" | "println" | "asleep" => return Type::Void,
                        "signal"   => return Type::Signal,
                        "contract" => return Type::Contract,
                        "permit"   => return Type::Permit,
                        name => {
                            if let Some(sig) = self.env.lookup_func_sig(name).cloned() {
                                if arg_types.len() != sig.params.len() {
                                    self.env.error(
                                        format!("function '{}' expects {} argument(s), got {}",
                                            name, sig.params.len(), arg_types.len()),
                                        c.span,
                                    );
                                } else {
                                    for (idx, (expected, got)) in sig.params.iter().zip(arg_types.iter()).enumerate() {
                                        if !Self::types_compatible(expected, got) {
                                            self.env.error(
                                                format!("argument {}: expected {}, got {}", idx + 1, expected, got),
                                                c.args[idx].span(),
                                            );
                                        }
                                    }
                                }
                                return if sig.is_async {
                                    Type::Future(Box::new(sig.return_type.clone()))
                                } else {
                                    sig.return_type.clone()
                                };
                            }
                        }
                    }
                }
                Type::Error
            }
            Expr::MethodCall(m) => self.check_method_call(m),
            Expr::FieldAccess(f) => self.check_field_access(f),
            Expr::Index(i) => {
                let obj_ty = self.check_expr(&i.object);
                self.check_expr(&i.index);
                match obj_ty {
                    Type::List(inner) => *inner,
                    _ => Type::Error,
                }
            }
            Expr::Await(a) => {
                let is_async_ctx = matches!(
                    self.env.current_func,
                    FuncContext::AsyncFunction { .. } | FuncContext::Handler { .. }
                );
                if !is_async_ctx {
                    self.env.error("await can only be used in async functions or handlers", a.span);
                }
                let inner = self.check_expr(&a.expr);
                match inner {
                    Type::Future(t) => *t,
                    Type::HandlerFuture(t) => *t,
                    Type::Signal | Type::Contract | Type::Permit => Type::Void,
                    _ => Type::Error,
                }
            }
            Expr::Panic(_) => Type::Never,
            Expr::Assert(_) => Type::Void,
            Expr::TypeCtor(tc) => self.check_type_ctor(tc),
            Expr::Try(t) => {
                let inner_ty = self.check_expr(&t.expr);
                Type::Result(Box::new(inner_ty), Box::new(Type::ErrorObj))
            }
        }
    }

    fn check_literal(&self, l: &Literal) -> Type {
        match &l.kind {
            LitKind::Bool(_) => Type::Bool,
            LitKind::Int(_) => Type::Int,
            LitKind::Double(_) => Type::Double,
            LitKind::Char(_) => Type::Char,
            LitKind::String(_) => Type::TString,
            LitKind::None => {
                if let Some(Type::Option(inner)) = &self.expected_ty {
                    Type::Option(inner.clone())
                } else {
                    Type::Option(Box::new(Type::Error))
                }
            }
        }
    }

    fn check_binop(&mut self, b: &BinOpExpr) -> Type {
        let lty = self.check_expr(&b.left);
        let rty = self.check_expr(&b.right);
        match b.op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                if lty == rty && lty.is_numeric() { lty }
                else { Type::Error }
            }
            BinOp::Eq | BinOp::Ne => Type::Bool,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                if lty == rty && lty.is_numeric() { Type::Bool }
                else { Type::Error }
            }
            BinOp::And | BinOp::Or => {
                if lty == Type::Bool && rty == Type::Bool { Type::Bool }
                else {
                    self.env.error("logical operators require bool operands", b.span);
                    Type::Error
                }
            }
        }
    }

    fn check_unary(&mut self, u: &UnaryOpExpr) -> Type {
        let inner = self.check_expr(&u.operand);
        match u.op {
            UnaryOp::Neg => {
                if inner.is_numeric() { inner }
                else { self.env.error("negation requires numeric type", u.span); Type::Error }
            }
            UnaryOp::Not => {
                if inner == Type::Bool { Type::Bool }
                else { self.env.error("logical not requires bool", u.span); Type::Error }
            }
        }
    }

    fn check_method_call(&mut self, m: &MethodCallExpr) -> Type {
        let recv_ty = self.check_expr(&m.receiver);
        for arg in &m.args { self.check_expr(arg); }
        match m.method.name.as_str() {
            "wait" => {
                match recv_ty {
                    Type::Future(inner) => *inner,
                    Type::HandlerFuture(inner) => *inner,
                    Type::Signal | Type::Contract | Type::Permit => Type::Void,
                    _ => Type::Error,
                }
            }
            "isDone" => {
                match recv_ty {
                    Type::Future(_) | Type::HandlerFuture(_) => Type::Bool,
                    _ => {
                        self.env.error(format!("isDone() is not defined on type {recv_ty}"), m.method.span);
                        Type::Error
                    }
                }
            }
            "isOk" if matches!(recv_ty, Type::HandlerFuture(_) | Type::Signal | Type::Contract | Type::Permit) => Type::Bool,
            "isErr" if matches!(recv_ty, Type::HandlerFuture(_) | Type::Signal | Type::Contract | Type::Permit) => Type::Bool,
            "raise" | "reset" => Type::Void,
            "isRaised" => Type::Bool,
            "awaitSignal" => Type::Void,
            "fulfill" => Type::Void,
            "isPending" => Type::Bool,
            "awaitContract" => Type::Void,
            "release" => Type::Void,
            "count" => {
                match recv_ty {
                    Type::Permit => Type::Int,
                    _ => Type::Error,
                }
            }
            "awaitPermit" => Type::Void,
            "terminate" => Type::Future(Box::new(Type::Void)),
            "length" => {
                match recv_ty {
                    Type::TString | Type::List(_) => Type::Int,
                    _ => {
                        self.env.error(format!("length() is not defined on type {recv_ty}"), m.method.span);
                        Type::Error
                    }
                }
            }
            "size" => {
                match recv_ty {
                    Type::Map(_, _) | Type::Queue(_) => Type::Int,
                    _ => {
                        self.env.error(format!("size() is not defined on type {recv_ty}"), m.method.span);
                        Type::Error
                    }
                }
            }
            "push" => {
                match recv_ty {
                    Type::List(_) => Type::Void,
                    Type::Queue(_) => Type::Result(
                        Box::new(Type::Void),
                        Box::new(Type::QueuePushError),
                    ),
                    _ => Type::Error,
                }
            }
            "pop" => {
                match recv_ty {
                    Type::List(inner) => Type::Option(inner),
                    _ => Type::Error,
                }
            }
            "set" | "close" => Type::Void,
            "remove" => {
                match recv_ty {
                    Type::Map(_, v) => Type::Option(v),
                    _ => Type::Error,
                }
            }
            "enqueue" | "waitForNonEmpty" => Type::Void,
            "dequeue" | "tryPop" => {
                match recv_ty {
                    Type::Queue(inner) => Type::Option(inner),
                    _ => Type::Error,
                }
            }
            "tryPush" => Type::Bool,
            "isEmpty" | "isClosed" => Type::Bool,
            "lock" | "unlock" => Type::Void,
            "tryLock" | "isLocked" => Type::Bool,
            "get" => {
                match recv_ty {
                    Type::List(inner) => *inner,
                    Type::Map(_, v) => Type::Option(v),
                    Type::Locked(inner) => *inner,
                    _ => Type::Error,
                }
            }
            "isSome" | "isNone" | "isOk" | "isErr" => Type::Bool,
            "unwrap" => {
                match recv_ty {
                    Type::Option(inner) => *inner,
                    Type::Result(ok, _) => *ok,
                    _ => Type::Error,
                }
            }
            "unwrapErr" => {
                match recv_ty {
                    Type::Result(_, err) => *err,
                    _ => Type::Error,
                }
            }
            "unwrapOr" => {
                match recv_ty {
                    Type::Option(inner) | Type::Result(inner, _) => *inner,
                    _ => Type::Error,
                }
            }
            // ── Type conversion methods ────────────────────────────────────────
            "toString" => {
                match recv_ty {
                    Type::Int | Type::Double | Type::Char | Type::Bool => Type::TString,
                    _ => {
                        self.env.error(format!("toString() is not defined on type {recv_ty}"), m.method.span);
                        Type::Error
                    }
                }
            }
            "toInt" => {
                match recv_ty {
                    Type::Double | Type::Char => Type::Int,
                    Type::TString => Type::Result(Box::new(Type::Int), Box::new(Type::ParseError)),
                    _ => {
                        self.env.error(format!("toInt() is not defined on type {recv_ty}"), m.method.span);
                        Type::Error
                    }
                }
            }
            "toDouble" => {
                match recv_ty {
                    Type::Int => Type::Double,
                    Type::TString => Type::Result(Box::new(Type::Double), Box::new(Type::ParseError)),
                    _ => {
                        self.env.error(format!("toDouble() is not defined on type {recv_ty}"), m.method.span);
                        Type::Error
                    }
                }
            }
            "toChar" => {
                match recv_ty {
                    Type::Int => Type::Option(Box::new(Type::Char)),
                    _ => {
                        self.env.error(format!("toChar() is not defined on type {recv_ty}"), m.method.span);
                        Type::Error
                    }
                }
            }
            _ => {
                if let Type::ThreadHandle(id) = recv_ty {
                    for (_, (tid, info)) in &self.env.templates {
                        if *tid == id {
                            if let Some(sig) = info.handlers.get(&m.method.name) {
                                return Type::HandlerFuture(Box::new(sig.return_type.clone()));
                            }
                        }
                    }
                    self.env.error(format!("unknown handler '{}' on thread handle", m.method.name), m.method.span);
                } else if recv_ty != Type::Error {
                    self.env.error(format!("unknown method '{}' on type {recv_ty}", m.method.name), m.method.span);
                }
                Type::Error
            }
        }
    }

    fn check_field_access(&mut self, f: &FieldAccessExpr) -> Type {
        let obj_ty = self.check_expr(&f.object);
        match obj_ty {
            Type::TemplateObject(ref fields) => {
                if let Some(ty) = fields.get(&f.field.name) {
                    return ty.clone();
                }
                self.env.error(format!("unknown field '{}' on self", f.field.name), f.field.span);
                Type::Error
            }
            Type::ThreadHandle(id) => {
                for (_, (tid, info)) in &self.env.templates {
                    if *tid == id {
                        if let Some(ei) = info.expose_fields.get(&f.field.name) {
                            return ei.ty.clone();
                        }
                    }
                }
                self.env.error(format!("unknown field '{}' on thread handle", f.field.name), f.field.span);
                Type::Error
            }
            Type::Error => Type::Error,
            Type::ErrorObj => match f.field.name.as_str() {
                "kind" | "message" => Type::TString,
                _ => {
                    self.env.error(format!("unknown field '{}' on error", f.field.name), f.field.span);
                    Type::Error
                }
            },
            _ => {
                self.env.error(format!("field access on non-object type {obj_ty}"), f.field.span);
                Type::Error
            }
        }
    }

    fn check_type_ctor(&mut self, tc: &TypeCtorExpr) -> Type {
        for arg in &tc.args { self.check_expr(arg); }
        match tc.name.as_str() {
            "Ok" => {
                let inner = tc.args.first().map(|a| self.check_expr(a)).unwrap_or(Type::Error);
                let err = if let Some(Type::Result(_, e)) = &self.expected_ty { *e.clone() } else { Type::Error };
                Type::Result(Box::new(inner), Box::new(err))
            }
            "Err" => {
                let inner = tc.args.first().map(|a| self.check_expr(a)).unwrap_or(Type::Error);
                let ok = if let Some(Type::Result(t, _)) = &self.expected_ty { *t.clone() } else { Type::Error };
                Type::Result(Box::new(ok), Box::new(inner))
            }
            "Some" => {
                let inner = tc.args.first().map(|a| self.check_expr(a)).unwrap_or(Type::Error);
                Type::Option(Box::new(inner))
            }
            "None" => {
                if let Some(Type::Option(inner)) = &self.expected_ty {
                    Type::Option(inner.clone())
                } else {
                    Type::Option(Box::new(Type::Error))
                }
            }
            "List" => {
                let elem = tc.type_args.first().map(|t| self.resolve_type(t)).unwrap_or(Type::Error);
                Type::List(Box::new(elem))
            }
            "Map" => {
                let k = tc.type_args.first().map(|t| self.resolve_type(t)).unwrap_or(Type::Error);
                let v = tc.type_args.get(1).map(|t| self.resolve_type(t)).unwrap_or(Type::Error);
                Type::Map(Box::new(k), Box::new(v))
            }
            "locked" => {
                let inner = tc.type_args.first().map(|t| self.resolve_type(t)).unwrap_or(Type::Error);
                Type::Locked(Box::new(inner))
            }
            "Queue" => {
                let inner = tc.type_args.first().map(|t| self.resolve_type(t)).unwrap_or(Type::Error);
                Type::Queue(Box::new(inner))
            }
            _ => Type::Error,
        }
    }
}
