//! L-HANDLER-PING-REDEFINED: every thread template implicitly carries
//! `async handler __ping__(): String` (R-HANDLER-PING). The runtime
//! intercepts that name at dispatch time and never enters the handler queue,
//! so a user-defined `__ping__` would be silently overridden. Surface that
//! collision as a hard error so the user notices.

use tessera_ast::*;
use tessera_types::TypeEnv;
use crate::{Diagnostic, LintPass};

pub struct HandlerPingRedefined;

impl LintPass for HandlerPingRedefined {
    fn name(&self) -> &'static str { "L-HANDLER-PING-REDEFINED" }
    fn check(&mut self, program: &Program, _env: &TypeEnv) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        for item in &program.items {
            if let TopLevelItem::ThreadTemplateDecl(d) = item {
                for m in &d.members {
                    if let ThreadTemplateMember::Handler(h) = m {
                        if h.name.name == "__ping__" {
                            diags.push(
                                Diagnostic::error(
                                    "L-HANDLER-PING-REDEFINED",
                                    "`__ping__` is an implicit virtual handler and cannot be overridden",
                                    h.span,
                                )
                                .with_help("rename this handler; the dispatch layer answers `__ping__` automatically with \"pong\""),
                            );
                        }
                    }
                }
            }
        }
        diags
    }
}
