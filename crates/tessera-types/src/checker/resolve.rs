//! Type-expression resolution: pure mapping from `TypeExpr` AST to `Type`.
//! Reused by every pass that needs a `Type` from a syntactic annotation.

use tessera_ast::*;
use crate::Type;
use super::TypeChecker;

impl<'e> TypeChecker<'e> {
    pub fn resolve_type(&mut self, te: &TypeExpr) -> Type {
        match te {
            TypeExpr::Void => Type::Void,
            TypeExpr::Never => Type::Never,
            TypeExpr::Named(ident, args) => self.resolve_named_type(&ident.name, args, ident.span),
        }
    }

    pub(super) fn resolve_named_type(&mut self, name: &str, args: &[TypeExpr], span: Span) -> Type {
        match name {
            "bool"   => Type::Bool,
            "int"    => Type::Int,
            "double" => Type::Double,
            "char"   => Type::Char,
            "String" => Type::TString,
            "void"   => Type::Void,
            "never"  => Type::Never,
            "List"   => {
                let inner = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::List(Box::new(inner))
            }
            "Map" => {
                let k = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                let v = args.get(1).map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::Map(Box::new(k), Box::new(v))
            }
            "Option" => {
                let inner = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::Option(Box::new(inner))
            }
            "Result" => {
                let t = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                let e = args.get(1).map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::Result(Box::new(t), Box::new(e))
            }
            "Future" => {
                let inner = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::Future(Box::new(inner))
            }
            "HandlerFuture" => {
                let inner = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::HandlerFuture(Box::new(inner))
            }
            "locked" => {
                let inner = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::Locked(Box::new(inner))
            }
            "Queue" => {
                let inner = args.first().map(|a| self.resolve_type(a)).unwrap_or(Type::Error);
                Type::Queue(Box::new(inner))
            }
            "signal" => Type::Signal,
            "contract" => Type::Contract,
            "permit" => Type::Permit,
            "HandlerDispatchError" => Type::HandlerDispatchError,
            "QueuePushError" => Type::QueuePushError,
            "ParseError" => Type::ParseError,
            "thread" => {
                if let Some(TypeExpr::Named(ident, _)) = args.first() {
                    if let Some((id, _)) = self.env.lookup_template(&ident.name) {
                        return Type::ThreadHandle(id);
                    }
                    self.env.error(format!("unknown thread template '{}'", ident.name), ident.span);
                } else {
                    self.env.error("thread<> requires a template name argument".to_string(), span);
                }
                Type::Error
            }
            other => {
                // Bare template name used directly as a type (legacy / shorthand)
                if let Some((id, _)) = self.env.lookup_template(other) {
                    Type::ThreadHandle(id)
                } else {
                    self.env.error(format!("unknown type '{other}'"), span);
                    Type::Error
                }
            }
        }
    }
}
