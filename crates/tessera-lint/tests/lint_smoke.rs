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
fn signal_wait_inside_shorthand_body_is_warned() {
    // `${ ... }` body runs in an async context, so a `.wait()` on a local
    // signal inside it should fire L-SIGNAL-WAIT-IN-ASYNC — same as inside
    // a named-template handler.
    let diags = run_lints(r#"
        ${
            let s: signal = signal();
            s.wait();
        };
    "#);
    assert!(has_rule(&diags, "L-SIGNAL-WAIT-IN-ASYNC"));
}

#[test]
fn method_chain_handler_call_is_warned() {
    // ScopedTyper now sees method-call return types, so a 3-step chain
    // `pool.tryPop().unwrap().ping();` resolves the receiver of `.ping()` to
    // `thread<Worker>` and the lint fires on the bare handler call.
    let diags = run_lints(r#"
        $template Worker() {
            async handler ping(): void {}
            async function __on_terminate__(): void {}
        }
        $Worker() { await keepalive(); } := w;
        let pool: Queue<thread<Worker>> = Queue<thread<Worker>>();
        pool.push(w);
        pool.tryPop().unwrap().ping();
    "#);
    assert!(has_rule(&diags, "L-HANDLER-RESULT-IGNORED"));
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

// ── L-EXCL-AWAIT ─────────────────────────────────────────────────────────────

#[test]
fn await_self_owned_signal_in_exclusive_is_warned() {
    let diags = run_lints(r#"
        $template Owner() {
            expose sig: signal;
            function __on_enter__(): void { self.sig = signal(); }
            async function __on_terminate__(): void {}
            async function loopit(): void {
                #exclusive {
                    await self.sig;
                }
            }
        }
    "#);
    assert!(has_rule(&diags, "L-EXCL-AWAIT"));
}

#[test]
fn wait_self_owned_permit_in_exclusive_is_warned() {
    let diags = run_lints(r#"
        $template Owner() {
            expose gate: permit;
            function __on_enter__(): void { self.gate = permit(0); }
            async function __on_terminate__(): void {}
            async function loopit(): void {
                #exclusive {
                    self.gate.wait();
                }
            }
        }
    "#);
    assert!(has_rule(&diags, "L-EXCL-AWAIT"));
}

#[test]
fn await_self_owned_signal_outside_exclusive_is_ok() {
    let diags = run_lints(r#"
        $template Owner() {
            expose sig: signal;
            function __on_enter__(): void { self.sig = signal(); }
            async function __on_terminate__(): void {}
            async function loopit(): void {
                await self.sig;
            }
        }
    "#);
    assert!(!has_rule(&diags, "L-EXCL-AWAIT"));
}

#[test]
fn await_other_threads_primitive_in_exclusive_is_ok() {
    // `self.o.sig` is another thread's primitive — R-EXCL-4 blesses it (that
    // thread drives it). The pass must not fire.
    let diags = run_lints(r#"
        $template Owner() {
            expose sig: signal;
            function __on_enter__(): void { self.sig = signal(); }
            async function __on_terminate__(): void {}
        }
        $template Waiter(o: thread<Owner>) {
            async function __on_terminate__(): void {}
            async function run(): void {
                #exclusive {
                    await self.o.sig;
                }
            }
        }
    "#);
    assert!(!has_rule(&diags, "L-EXCL-AWAIT"));
}

// ── L-RETURN-TYPE-MISMATCH ───────────────────────────────────────────────────

#[test]
fn bare_return_in_value_function_is_rejected() {
    let diags = run_lints("function f(): int { return; }");
    assert!(has_rule(&diags, "L-RETURN-TYPE-MISMATCH"));
}

#[test]
fn literal_return_type_mismatch_is_rejected() {
    let diags = run_lints(r#"function g(): int { return "nope"; }"#);
    assert!(has_rule(&diags, "L-RETURN-TYPE-MISMATCH"));
}

#[test]
fn matching_literal_return_is_ok() {
    let diags = run_lints("function f(): int { return 5; }");
    assert!(!has_rule(&diags, "L-RETURN-TYPE-MISMATCH"));
}

#[test]
fn int_literal_for_double_return_is_ok() {
    // The language tolerates the int ⇆ double coercion, so this must not fire.
    let diags = run_lints("function f(): double { return 5; }");
    assert!(!has_rule(&diags, "L-RETURN-TYPE-MISMATCH"));
}

#[test]
fn non_literal_return_is_not_judged() {
    // A non-literal expression's type is left to the type checker; the lint
    // must stay silent to remain sound (zero false positives).
    let diags = run_lints(r#"function f(): String { return "a" + "b"; }"#);
    assert!(!has_rule(&diags, "L-RETURN-TYPE-MISMATCH"));
}

// ── L-VOID-RETURN-VALUE ──────────────────────────────────────────────────────

#[test]
fn returning_value_from_void_is_rejected() {
    let diags = run_lints("function f(): void { return 1; }");
    assert!(has_rule(&diags, "L-VOID-RETURN-VALUE"));
}

#[test]
fn bare_return_in_void_is_ok() {
    let diags = run_lints("function f(): void { return; }");
    assert!(!has_rule(&diags, "L-VOID-RETURN-VALUE"));
    assert!(!has_rule(&diags, "L-RETURN-TYPE-MISMATCH"));
}

// ── L-EXPOSE-READONLY-CONTAINER ──────────────────────────────────────────────

#[test]
fn expose_readonly_list_is_flagged() {
    let diags = run_lints(r#"
        $template W() {
            expose items: List<int>;
            async function __on_terminate__(): void {}
        }
    "#);
    assert!(has_rule(&diags, "L-EXPOSE-READONLY-CONTAINER"));
}

#[test]
fn expose_readonly_scalar_is_ok() {
    let diags = run_lints(r#"
        $template W() {
            expose count: int;
            expose handle: locked<int>;
            async function __on_terminate__(): void {}
        }
    "#);
    assert!(!has_rule(&diags, "L-EXPOSE-READONLY-CONTAINER"));
}

// ── L-GENERIC-NESTING-DEPTH ──────────────────────────────────────────────────

#[test]
fn deeply_nested_generic_is_flagged() {
    let diags = run_lints("function f(x: List<List<List<List<int>>>>): void {}");
    assert!(has_rule(&diags, "L-GENERIC-NESTING-DEPTH"));
}

#[test]
fn shallow_generic_is_ok() {
    // `List<List<int>>` is depth 3 — at the threshold, not over it.
    let diags = run_lints("function f(x: List<List<int>>): void {}");
    assert!(!has_rule(&diags, "L-GENERIC-NESTING-DEPTH"));
}

// ── L-CONTROL-OUTSIDE-LOOP ───────────────────────────────────────────────────

#[test]
fn break_in_function_outside_loop_is_rejected() {
    let diags = run_lints("function f(): void { break; }");
    assert!(has_rule(&diags, "L-CONTROL-OUTSIDE-LOOP"));
}

#[test]
fn continue_at_top_level_is_rejected() {
    let diags = run_lints("continue;");
    assert!(has_rule(&diags, "L-CONTROL-OUTSIDE-LOOP"));
}

#[test]
fn break_inside_loop_in_function_is_ok() {
    let diags = run_lints("function f(): void { while (true) { if (true) { break; } } }");
    assert!(!has_rule(&diags, "L-CONTROL-OUTSIDE-LOOP"));
}

#[test]
fn break_in_handler_outside_loop_is_rejected() {
    let diags = run_lints(r#"
        $template W() {
            async function __on_terminate__(): void {}
            async handler h(): void { break; }
        }
    "#);
    assert!(has_rule(&diags, "L-CONTROL-OUTSIDE-LOOP"));
}

// ── L-VOID-RETURN-VALUE: thread body is void-like ────────────────────────────

#[test]
fn return_value_from_thread_body_is_rejected() {
    let diags = run_lints(r#"
        $template W() { async function __on_terminate__(): void {} }
        $W() { return 5; } := h;
    "#);
    assert!(has_rule(&diags, "L-VOID-RETURN-VALUE"));
}

#[test]
fn bare_return_in_thread_body_is_ok() {
    let diags = run_lints(r#"
        $template W() { async function __on_terminate__(): void {} }
        $W() { return; } := h;
    "#);
    assert!(!has_rule(&diags, "L-VOID-RETURN-VALUE"));
}
