use tessera_ast::*;
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
            "void" => Type::Void,
            "never" => Type::Never,
            "List" => Type::List(Box::new(
                args.first().map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)
            )),
            "Map" => Type::Map(
                Box::new(args.first().map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)),
                Box::new(args.get(1).map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)),
            ),
            "Option" => Type::Option(Box::new(
                args.first().map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)
            )),
            "Result" => Type::Result(
                Box::new(args.first().map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)),
                Box::new(args.get(1).map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)),
            ),
            "Future" => Type::Future(Box::new(
                args.first().map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)
            )),
            "HandlerFuture" => Type::HandlerFuture(Box::new(
                args.first().map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)
            )),
            "locked" => Type::Locked(Box::new(
                args.first().map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)
            )),
            "Queue" => Type::Queue(Box::new(
                args.first().map(|a| resolve_type_expr(env, a)).unwrap_or(Type::Error)
            )),
            "signal" => Type::Signal,
            "contract" => Type::Contract,
            "permit" => Type::Permit,
            "thread" => match args.first() {
                Some(TypeExpr::Named(tn, _)) => env.templates.get(&tn.name)
                    .map(|(id, _)| Type::ThreadHandle(*id))
                    .unwrap_or(Type::Error),
                _ => Type::Error,
            },
            // Bare template name used as a type, e.g. `worker` rather than
            // `thread<worker>` (parser allows this shorthand).
            other => env.templates.get(other)
                .map(|(id, _)| Type::ThreadHandle(*id))
                .unwrap_or(Type::Error),
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

/// Visit every top-level *function-like unit* (free functions, scope/thread
/// template member functions, lifecycle hooks, and handlers), calling `f` with
/// the unit's declared return type and body. Anonymous templates nested inside
/// statements are not visited — passes built on this stay sound (they simply
/// lint fewer sites) and mirror `return_not_all_paths`'s coverage.
pub(crate) fn each_function(program: &Program, f: &mut impl FnMut(&TypeExpr, &Block)) {
    for item in &program.items {
        match item {
            TopLevelItem::FuncDef(fd) => f(&fd.return_type, &fd.body),
            TopLevelItem::ScopeTemplateDecl(d) => {
                for m in &d.members {
                    match m {
                        ScopeTemplateMember::OnEnter(fd)
                        | ScopeTemplateMember::OnExit(fd)
                        | ScopeTemplateMember::MemberFunc(fd) => f(&fd.return_type, &fd.body),
                        ScopeTemplateMember::Define(_) => {}
                    }
                }
            }
            TopLevelItem::ThreadTemplateDecl(d) => {
                for m in &d.members {
                    match m {
                        ThreadTemplateMember::OnEnter(fd)
                        | ThreadTemplateMember::OnExit(fd)
                        | ThreadTemplateMember::OnTerminate(fd)
                        | ThreadTemplateMember::MemberFunc(fd) => f(&fd.return_type, &fd.body),
                        ThreadTemplateMember::Handler(h) => f(&h.return_type, &h.body),
                        _ => {}
                    }
                }
            }
            TopLevelItem::Statement(_) => {}
        }
    }
}

/// Visit every `return` statement reachable from `block` *within the same
/// function/handler execution context* — recursing through control-flow,
/// scope, and `#exclusive` blocks, but never descending into nested
/// thread-spawn bodies (those run in a different thread with their own return
/// context, governed by the top-level / spawn-body rules instead).
pub(crate) fn each_return(block: &Block, f: &mut impl FnMut(&ReturnStmt)) {
    for s in &block.stmts {
        each_return_stmt(s, f);
    }
}

fn each_return_stmt(s: &Stmt, f: &mut impl FnMut(&ReturnStmt)) {
    match s {
        Stmt::Return(r) => f(r),
        Stmt::If(i) => each_return_if(i, f),
        Stmt::While(w) => each_return(&w.body, f),
        Stmt::For(fo) => {
            if let Some(init) = &fo.init { each_return_stmt(init, f); }
            if let Some(upd) = &fo.update { each_return_stmt(upd, f); }
            each_return(&fo.body, f);
        }
        Stmt::ScopeBlock(sb) => each_return(&sb.body, f),
        Stmt::ExclusiveBlock(eb) => each_return(&eb.body, f),
        // ThreadSpawn body runs in a different thread context — skip it.
        _ => {}
    }
}

fn each_return_if(i: &IfStmt, f: &mut impl FnMut(&ReturnStmt)) {
    each_return(&i.then_block, f);
    match &i.else_branch {
        Some(ElseBranch::Else(b)) => each_return(b, f),
        Some(ElseBranch::ElseIf(i2)) => each_return_if(i2, f),
        None => {}
    }
}

/// Nesting depth of a type expression: a leaf (no type args) is depth 1, and a
/// constructor's depth is `1 + max(depth(arg))`. Used by L-GENERIC-NESTING-DEPTH.
pub(crate) fn type_expr_depth(te: &TypeExpr) -> usize {
    match te {
        TypeExpr::Void | TypeExpr::Never => 1,
        TypeExpr::Named(_, args) => {
            1 + args.iter().map(type_expr_depth).max().unwrap_or(0)
        }
    }
}

