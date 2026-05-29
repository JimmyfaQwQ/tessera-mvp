//! L-EXPOSE-READONLY-CONTAINER (Info): a thread template `expose`s (read-only)
//! a non-concurrent-safe container (`List<T>` / `Map<K, V>`).
//!
//! Such a field is readable through the thread handle from another thread, but
//! `List` / `Map` are backed by `Rc` (not `Send`): a genuine cross-thread read
//! surfaces as a low-level Rust error rather than a Tessera-domain error. The
//! concurrency-safe story is `locked<T>` / `Queue<T>`. `expose_mutable` of an
//! unsafe type is already a hard error (L-EXPOSE-MUTABLE-UNSAFE); the read-only
//! side is advisory only, hence Info.
//!
//! Anchors 《数据共享与并发安全规范 §3.2 / §5》 and `spec-alignment` 偏差 1.

use tessera_ast::*;
use tessera_types::{Type, TypeEnv};

use crate::{Diagnostic, LintPass};
use super::helpers::resolve_type_expr;

pub struct ExposeReadonlyContainer;

impl LintPass for ExposeReadonlyContainer {
    fn name(&self) -> &'static str { "L-EXPOSE-READONLY-CONTAINER" }
    fn check(&mut self, program: &Program, env: &TypeEnv) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        for item in &program.items {
            if let TopLevelItem::ThreadTemplateDecl(d) = item {
                for m in &d.members {
                    // Only the read-only `expose` form; `expose_mutable` is
                    // covered (as an error) by L-EXPOSE-MUTABLE-UNSAFE.
                    if let ThreadTemplateMember::Expose(e) = m {
                        let ty = resolve_type_expr(env, &e.ty);
                        if matches!(ty, Type::List(_) | Type::Map(_, _)) {
                            diags.push(
                                Diagnostic::info(
                                    "L-EXPOSE-READONLY-CONTAINER",
                                    format!(
                                        "`expose` field `{}` has non-concurrent-safe container type `{ty}`; cross-thread reads are not safe",
                                        e.name.name
                                    ),
                                    e.span,
                                )
                                .with_help("wrap shared state in `locked<T>` or `Queue<T>`, or keep the container `define`-private"),
                            );
                        }
                    }
                }
            }
        }
        diags
    }
}
