use tessera_ast::TypeExpr;
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
