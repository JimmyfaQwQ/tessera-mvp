use tessera_ast::{Expr, TypeExpr};
use tessera_types::{Type, TypeEnv};

/// Resolve a syntactic type expression into the semantic `Type` used by the
/// type environment. Shared by lint passes that need to compare a field's
/// type against `Type::*` enum variants (e.g. `is_concurrent_safe`).
#[allow(clippy::only_used_in_recursion)]
pub(crate) fn resolve_type_expr(env: &TypeEnv, te: &TypeExpr) -> Type {
    match te {
        TypeExpr::Void => Type::Void,
        TypeExpr::Never => Type::Never,
        TypeExpr::Named(ident, args) => match ident.name.as_str() {
            "bool" => Type::Bool,
            "int" => Type::Int,
            "double" => Type::Double,
            "char" => Type::Char,
            "String" => Type::TString,
            "locked" => Type::Locked(Box::new(
                args.first().map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)
            )),
            "Queue" => Type::Queue(Box::new(
                args.first().map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)
            )),
            "signal" => Type::Signal,
            "contract" => Type::Contract,
            "permit" => Type::Permit,
            _ => Type::Error,
        },
    }
}

/// Lightweight, "good-enough" expression-type inference for lint passes.
///
/// Covers the cases lints actually need: identifiers (from the top-level
/// scope after type-check), field access through `ThreadHandle` and
/// `TemplateObject`, and the standard constructors (`Ok` / `Err` / `Some` /
/// `None` / `List` / `Map` / `Result` / `Option` / `Future` / `HandlerFuture`
/// / `locked` / `Queue` / `signal` / `contract` / `permit`). Everything else
/// returns `None` so callers can conservatively skip.
///
/// Note: locals declared inside function / handler bodies are popped from the
/// env after type-checking finishes, so identifier lookup only succeeds for
/// top-level bindings or names bound in still-active enclosing scopes.
pub(crate) fn infer_expr_type(env: &TypeEnv, e: &Expr) -> Option<Type> {
    match e {
        Expr::Ident(i) => env.lookup(&i.name).cloned(),

        Expr::FieldAccess(fa) => {
            let obj = infer_expr_type(env, &fa.object)?;
            match obj {
                Type::ThreadHandle(id) => {
                    let info = env.templates.values()
                        .find(|(tid, _)| *tid == id)
                        .map(|(_, info)| info)?;
                    info.expose_fields.get(&fa.field.name).map(|ex| ex.ty.clone())
                }
                Type::TemplateObject(fields) => fields.get(&fa.field.name).cloned(),
                _ => None,
            }
        }

        Expr::TypeCtor(tc) => match tc.name.as_str() {
            "Some" => tc.args.first().and_then(|a| infer_expr_type(env, a))
                .map(|t| Type::Option(Box::new(t))),
            "None" => Some(Type::Option(Box::new(Type::Error))),
            "Ok"   => tc.args.first().and_then(|a| infer_expr_type(env, a))
                .map(|t| Type::Result(Box::new(t), Box::new(Type::Error))),
            "Err"  => tc.args.first().and_then(|a| infer_expr_type(env, a))
                .map(|e| Type::Result(Box::new(Type::Error), Box::new(e))),
            "List" => {
                let inner = tc.type_args.first().map(|t| resolve_type_expr(env, t))
                    .unwrap_or(Type::Error);
                Some(Type::List(Box::new(inner)))
            }
            "Queue" => {
                let inner = tc.type_args.first().map(|t| resolve_type_expr(env, t))
                    .unwrap_or(Type::Error);
                Some(Type::Queue(Box::new(inner)))
            }
            "locked" => tc.args.first().and_then(|a| infer_expr_type(env, a))
                .map(|t| Type::Locked(Box::new(t))),
            _ => None,
        },

        _ => None,
    }
}

