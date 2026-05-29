//! Smoke tests for the lint passes added under the spec-alignment fix plan.
//!
//! Each test runs lex → parse → type-check → lint and asserts that the
//! expected rule fires (or does not fire on a positive example).

use tessera_lexer::lex;
use tessera_lint::{LintRunner, Severity};
use tessera_parser::parse;
use tessera_types::{TypeChecker, TypeEnv};

fn run_lints(src: &str) -> Vec<(String, Severity)> {
    let tokens = lex(src);
    let (program, _parse_errors) = parse(tokens);
    let mut env = TypeEnv::new();
    TypeChecker::new(&mut env).check_program(&program);
    let mut runner = LintRunner::default_passes();
    runner
        .run_all(&program, &env)
        .into_iter()
        .map(|d| (d.rule_id.to_string(), d.severity))
        .collect()
}

fn has_rule(diags: &[(String, Severity)], id: &str) -> bool {
    diags.iter().any(|(rid, _)| rid == id)
}

// ── L-TOPLEVEL-CONTROL-FLOW ──────────────────────────────────────────────────

#[test]
fn toplevel_return_is_rejected() {
    let diags = run_lints("return;");
    assert!(has_rule(&diags, "L-TOPLEVEL-CONTROL-FLOW"));
}

#[test]
fn toplevel_await_is_rejected() {
    let diags = run_lints(r#"
        $template W() { async function __on_terminate__(): void {} }
        $W() { await keepalive(); } := h;
        let p: String = await h.__ping__();
    "#);
    assert!(has_rule(&diags, "L-TOPLEVEL-CONTROL-FLOW"));
}

#[test]
fn toplevel_break_inside_loop_is_ok() {
    let diags = run_lints("while (true) { break; }");
    assert!(!has_rule(&diags, "L-TOPLEVEL-CONTROL-FLOW"));
}

// ── L-FUNCTION-HOOK-SIGNATURE ────────────────────────────────────────────────

#[test]
fn on_enter_must_be_sync_void() {
    let diags = run_lints(r#"
        @template Foo() {
            async function __on_enter__(): void {}
        }
    "#);
    assert!(has_rule(&diags, "L-FUNCTION-HOOK-SIGNATURE"));
}

#[test]
fn on_terminate_must_be_async() {
    let diags = run_lints(r#"
        $template Bar() {
            function __on_terminate__(): void {}
        }
    "#);
    assert!(has_rule(&diags, "L-FUNCTION-HOOK-SIGNATURE"));
}

// ── L-HANDLER-PING-REDEFINED ─────────────────────────────────────────────────

#[test]
fn explicit_ping_handler_is_rejected() {
    let diags = run_lints(r#"
        $template W() {
            async handler __ping__(): String { return "custom"; }
            async function __on_terminate__(): void {}
        }
    "#);
    assert!(has_rule(&diags, "L-HANDLER-PING-REDEFINED"));
}

// ── L-EXPOSE-READONLY-WRITE ──────────────────────────────────────────────────

#[test]
fn write_to_readonly_expose_is_rejected() {
    let diags = run_lints(r#"
        $template W() {
            expose count: int;
            async function __on_terminate__(): void {}
        }
        $W() { await keepalive(); } := h;
        h.count = 5;
    "#);
    assert!(has_rule(&diags, "L-EXPOSE-READONLY-WRITE"));
}

// ── L-DEFINE-EXTERNAL-ACCESS ─────────────────────────────────────────────────

#[test]
fn external_define_access_is_rejected() {
    let diags = run_lints(r#"
        $template W() {
            define secret: int = 1;
            async function __on_terminate__(): void {}
        }
        $W() { await keepalive(); } := h;
        let x: int = h.secret;
    "#);
    assert!(has_rule(&diags, "L-DEFINE-EXTERNAL-ACCESS"));
}

// ── L-RETURN-NOT-ALL-PATHS ───────────────────────────────────────────────────

#[test]
fn missing_return_path_is_rejected() {
    let diags = run_lints(r#"
        function f(c: bool): int {
            if (c) { return 1; }
        }
    "#);
    assert!(has_rule(&diags, "L-RETURN-NOT-ALL-PATHS"));
}

#[test]
fn all_paths_return_is_ok() {
    let diags = run_lints(r#"
        function f(c: bool): int {
            if (c) { return 1; }
            return 0;
        }
    "#);
    assert!(!has_rule(&diags, "L-RETURN-NOT-ALL-PATHS"));
}

// ── L-SIGNAL-AWAIT-IN-SYNC / L-CONTRACT-AWAIT-IN-SYNC ────────────────────────

#[test]
fn signal_await_in_sync_is_rejected() {
    let diags = run_lints(r#"
        let s: signal = signal();
        let r = try await s;
    "#);
    assert!(has_rule(&diags, "L-SIGNAL-AWAIT-IN-SYNC"));
}

#[test]
fn contract_await_in_sync_is_rejected() {
    let diags = run_lints(r#"
        let c: contract = contract();
        let r = try await c;
    "#);
    assert!(has_rule(&diags, "L-CONTRACT-AWAIT-IN-SYNC"));
}

// ── L-SIGNAL-WAIT-IN-ASYNC / L-CONTRACT-WAIT-IN-ASYNC ────────────────────────

#[test]
fn signal_wait_in_async_is_warned() {
    // Locals inside function bodies are popped from the env after the type
    // checker finishes, so the lint can only see types of identifiers that
    // are still in the top-level scope. Bind `s` at the top level and consume
    // it inside a thread-spawn body (which the visitor treats as async).
    let diags = run_lints(r#"
        let s: signal = signal();
        ${ s.wait(); };
    "#);
    assert!(has_rule(&diags, "L-SIGNAL-WAIT-IN-ASYNC"));
}

#[test]
fn contract_wait_in_async_is_warned() {
    let diags = run_lints(r#"
        let c: contract = contract();
        ${ c.wait(); };
    "#);
    assert!(has_rule(&diags, "L-CONTRACT-WAIT-IN-ASYNC"));
}

#[test]
fn signal_wait_in_sync_is_ok() {
    let diags = run_lints(r#"
        let s: signal = signal();
        s.wait();
    "#);
    assert!(!has_rule(&diags, "L-SIGNAL-WAIT-IN-ASYNC"));
}

// ── L-HANDLER-RESULT-IGNORED ─────────────────────────────────────────────────

#[test]
fn bare_handler_call_is_warned() {
    let diags = run_lints(r#"
        $template W() {
            async handler do_it(): void {}
            async function __on_terminate__(): void {}
        }
        $W() { await keepalive(); } := h;
        h.do_it();
    "#);
    assert!(has_rule(&diags, "L-HANDLER-RESULT-IGNORED"));
}

#[test]
fn local_let_handler_call_is_warned() {
    // ScopedTyper sees `p` bound by the inner `let`; without it the lint
    // missed this fire-and-forget pattern.
    let diags = run_lints(r#"
        $template W() {
            async handler do_it(): void {}
            async function __on_terminate__(): void {}
        }
        $template Outer() {
            async handler dispatch(): void {
                $W() { await keepalive(); } := p;
                p.do_it();
            }
            async function __on_terminate__(): void {}
        }
    "#);
    assert!(has_rule(&diags, "L-HANDLER-RESULT-IGNORED"));
}

#[test]
fn local_signal_wait_in_async_is_warned() {
    let diags = run_lints(r#"
        $template T() {
            async function __on_terminate__(): void {}
            async function run(): void {
                let s: signal = signal();
                s.wait();
            }
        }
    "#);
    assert!(has_rule(&diags, "L-SIGNAL-WAIT-IN-ASYNC"));
}

#[test]
fn local_contract_wait_in_async_is_warned() {
    let diags = run_lints(r#"
        $template T() {
            async function __on_terminate__(): void {}
            async function run(): void {
                let c: contract = contract();
                c.wait();
            }
        }
    "#);
    assert!(has_rule(&diags, "L-CONTRACT-WAIT-IN-ASYNC"));
}

#[test]
fn local_permit_wait_in_async_is_warned() {
    let diags = run_lints(r#"
        $template T() {
            async function __on_terminate__(): void {}
            async function run(): void {
                let p: permit = permit(0);
                p.wait();
            }
        }
    "#);
    assert!(has_rule(&diags, "L-PERMIT-WAIT-IN-ASYNC"));
}

#[test]
fn local_signal_wait_in_sync_is_ok() {
    // Same shape but in a sync function — no warning expected.
    let diags = run_lints(r#"
        $template T() {
            async function __on_terminate__(): void {}
            function run(): void {
                let s: signal = signal();
                s.wait();
            }
        }
    "#);
    assert!(!has_rule(&diags, "L-SIGNAL-WAIT-IN-ASYNC"));
}

#[test]
fn field_chain_handler_call_is_warned() {
    // FieldAccess receiver — exercises the new lightweight typer.
    let diags = run_lints(r#"
        $template Inner() {
            async handler tick(): void {}
            async function __on_terminate__(): void {}
        }
        $template Outer() {
            expose inner: thread<Inner>;
            async function __on_terminate__(): void {}
        }
        $Outer() { await keepalive(); } := o;
        o.inner.tick();
    "#);
    assert!(has_rule(&diags, "L-HANDLER-RESULT-IGNORED"));
}
