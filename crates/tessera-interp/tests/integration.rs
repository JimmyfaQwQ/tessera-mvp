use tessera_lexer::lex;
use tessera_parser::parse;
use tessera_interp::run;
use tessera_runtime::RuntimeError;

async fn run_src(src: &str) -> Result<(), RuntimeError> {
    let tokens = lex(src);
    let (program, _errors) = parse(tokens);
    run(&program).await
}

macro_rules! assert_runs_ok {
    ($src:expr) => {{
        let result = run_src($src).await;
        if let Err(ref e) = result {
            panic!("expected Ok, got error: {}", e);
        }
        result.unwrap()
    }};
}

macro_rules! assert_runs_err {
    ($src:expr) => {{
        let result = run_src($src).await;
        assert!(result.is_err(), "expected Err, got Ok");
        result.unwrap_err()
    }};
}

// ── Helper that loads a .tss test file ───────────────────────────────────────

fn tss(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/tss/{}.tss",
        env!("CARGO_MANIFEST_DIR"),
        name
    ))
    .unwrap_or_else(|e| panic!("failed to read {}.tss: {}", name, e))
}

// ── User-defined functions ────────────────────────────────────────────────────

#[tokio::test]
async fn test_user_func() {
    let src = tss("user_func");
    assert_runs_ok!(&src);
}

// ── Scope template hooks ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_scope_hooks() {
    let src = tss("scope_hooks");
    assert_runs_ok!(&src);
}

// ── Scope binding: define fields go Broken on scope exit ─────────────────────

#[tokio::test]
async fn test_scope_binding() {
    let src = tss("scope_binding");
    assert_runs_ok!(&src);
}

// ── Thread lifecycle (spawn, body runs, expose read) ─────────────────────────

#[tokio::test]
async fn test_thread_lifecycle() {
    let src = tss("thread_lifecycle");
    assert_runs_ok!(&src);
}

// ── Exclusive block ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_exclusive_block() {
    let src = tss("exclusive_block");
    assert_runs_ok!(&src);
}

// ── locked<T> shared between threads ─────────────────────────────────────────

#[tokio::test]
async fn test_locked_shared() {
    let src = tss("locked_shared");
    assert_runs_ok!(&src);
}

// ── R-HANDLER-PING: virtual __ping__ injected on every thread template ───────

#[tokio::test]
async fn test_ping_handler() {
    let src = tss("ping_handler");
    assert_runs_ok!(&src);
}

// ── R-HANDLER-2: handler-handler mutual exclusion ────────────────────────────

#[tokio::test]
async fn test_handler_mutex() {
    let src = tss("handler_mutex");
    assert_runs_ok!(&src);
}

// ── Anonymous shorthand thread `${ ... }` ────────────────────────────────────

#[tokio::test]
async fn test_anonymous_thread() {
    let src = tss("anonymous_thread");
    assert_runs_ok!(&src);
}

// ── Standard library extensions (round 3) ────────────────────────────────────

#[tokio::test]
async fn test_string_starts_ends_with() {
    assert_runs_ok!(r#"
        let s: String = "hello world";
        assert(s.startsWith("hello"));
        assert(!s.startsWith("world"));
        assert(s.endsWith("world"));
        assert(!s.endsWith("hello"));
        assert("".startsWith(""));
    "#);
}

#[tokio::test]
async fn test_string_contains_index_of() {
    assert_runs_ok!(r#"
        let s: String = "héllo";
        assert(s.contains("ll"));
        assert(!s.contains("xyz"));
        // indexOf returns Unicode-scalar index, not byte offset
        assert(s.indexOf("ll") == 2);
        assert(s.indexOf("xyz") == -1);
    "#);
}

#[tokio::test]
async fn test_string_trim() {
    assert_runs_ok!(r#"
        let s: String = "  hello\n";
        assert(s.trim() == "hello");
        assert("nothing".trim() == "nothing");
    "#);
}

#[tokio::test]
async fn test_string_split() {
    assert_runs_ok!(r#"
        let parts: List<String> = "a,b,c".split(",");
        assert(parts.length() == 3);
        assert(parts.get(0) == "a");
        assert(parts.get(2) == "c");
        let chars: List<String> = "ab".split("");
        assert(chars.length() == 2);
    "#);
}

#[tokio::test]
async fn test_list_contains_index_of() {
    assert_runs_ok!(r#"
        let xs: List<int> = List<int>(1, 2, 3);
        assert(xs.contains(2));
        assert(!xs.contains(99));
        assert(xs.indexOf(3) == 2);
        assert(xs.indexOf(99) == -1);
    "#);
}

#[tokio::test]
async fn test_list_clear() {
    assert_runs_ok!(r#"
        let xs: List<int> = List<int>(1, 2, 3);
        xs.clear();
        assert(xs.length() == 0);
        assert(xs.isEmpty());
    "#);
}

#[tokio::test]
async fn test_map_contains() {
    assert_runs_ok!(r#"
        let m: Map<String, int> = Map<String, int>();
        m.set("a", 1);
        assert(m.contains("a"));
        assert(!m.contains("b"));
    "#);
}

#[tokio::test]
async fn test_char_classification() {
    assert_runs_ok!(r#"
        assert('5'.isDigit());
        assert(!'a'.isDigit());
        assert('a'.isAlpha());
        assert('Z'.isAlpha());
        assert(!'5'.isAlpha());
        assert(' '.isWhitespace());
        assert('\t'.isWhitespace());
        assert(!'x'.isWhitespace());
    "#);
}

// ── R-SYNC-BREAK-3: no deadlock when Broken arrives inside `#exclusive` ──────

#[tokio::test]
async fn test_exclusive_broken_wait() {
    let src = tss("exclusive_broken_wait");
    assert_runs_ok!(&src);
}

// ── Inline expression tests ───────────────────────────────────────────────────

#[tokio::test]
async fn test_arithmetic() {
    assert_runs_ok!("let x: int = 2 + 3 * 4; assert(x == 14);");
}

#[tokio::test]
async fn test_string_concat() {
    assert_runs_ok!(r#"let s: String = "hello" + " world"; assert(s == "hello world");"#);
}

#[tokio::test]
async fn test_option_some() {
    assert_runs_ok!("let o: Option<int> = Some(42); assert(o.isSome()); assert(o.unwrap() == 42);");
}

#[tokio::test]
async fn test_result_ok() {
    assert_runs_ok!("let r: Result<int, String> = Ok(7); assert(r.isOk()); assert(r.unwrap() == 7);");
}

#[tokio::test]
async fn test_list_push_pop() {
    assert_runs_ok!("
        let xs: List<int> = List<int>(1, 2, 3);
        xs.push(4);
        assert(xs.length() == 4);
        assert(xs.pop().unwrap() == 4);
    ");
}

#[tokio::test]
async fn test_while_loop() {
    assert_runs_ok!("
        let i: int = 0;
        let sum: int = 0;
        while (i < 5) {
            sum = sum + i;
            i = i + 1;
        }
        assert(sum == 10);
    ");
}

#[tokio::test]
async fn test_assert_failure() {
    let err = assert_runs_err!("assert(1 == 2, \"one != two\");");
    assert!(matches!(err, RuntimeError::AssertionFailed { .. }));
}

#[tokio::test]
async fn test_divide_by_zero() {
    let err = assert_runs_err!("let x: int = 1 / 0;");
    assert!(matches!(err, RuntimeError::DivisionByZero { .. }));
}

#[tokio::test]
async fn test_unwrap_none() {
    let err = assert_runs_err!("let o: Option<int> = None; o.unwrap();");
    assert!(matches!(err, RuntimeError::UnwrapNone { .. }));
}

// A failing thread-template field initializer must crash the thread (surfaced
// here via `terminate().wait()`) rather than silently defaulting the field.
#[tokio::test]
async fn test_thread_field_initializer_failure_crashes() {
    let err = assert_runs_err!(r#"
        $template Bad() {
            define x: int = 1 / 0;
            async function __on_terminate__(): void {}
        }
        $Bad() { let y: int = 5; } := handle;
        handle.terminate().wait();
    "#);
    assert!(matches!(err, RuntimeError::Panic { .. }));
}

// When a scope body succeeds but `__on_exit__` fails, the on_exit error is
// surfaced (previously it was silently swallowed).
#[tokio::test]
async fn test_scope_on_exit_error_surfaced() {
    let err = assert_runs_err!(r#"
        @template C() {
            function __on_exit__(): void { let z: int = 1 / 0; }
        }
        @C() { let y: int = 5; }
    "#);
    assert!(matches!(err, RuntimeError::DivisionByZero { .. }));
}

// When both the scope body and `__on_exit__` fail, the body error (which
// happened first) takes precedence over the on_exit error.
#[tokio::test]
async fn test_scope_body_error_precedes_on_exit_error() {
    let err = assert_runs_err!(r#"
        @template C() {
            function __on_exit__(): void { panic("exit failed"); }
        }
        @C() { let z: int = 7 / 0; }
    "#);
    assert!(matches!(err, RuntimeError::DivisionByZero { .. }));
}

#[tokio::test]
async fn test_recursive_fib() {
    assert_runs_ok!("
        function fib(n: int): int {
            if (n <= 1) { return n; }
            return fib(n - 1) + fib(n - 2);
        }
        assert(fib(0) == 0);
        assert(fib(1) == 1);
        assert(fib(7) == 13);
    ");
}

// §1.1.5: `String[i]` returns the i-th Unicode scalar, and `\u{HHHH}` decodes
// to a Unicode codepoint in both string and char literals (P1-4).
#[tokio::test]
async fn test_string_indexing_and_unicode() {
    assert_runs_ok!(r#"
        let s: String = "héllo";
        assert(s[0] == 'h');
        assert(s[1] == 'é');
        assert(s.length() == 5);
        let smiley: char = '\u{1F600}';
        assert(smiley == '\u{1F600}');
    "#);
}

#[tokio::test]
async fn test_string_index_out_of_bounds() {
    let err = assert_runs_err!(r#"
        let s: String = "abc";
        let c: char = s[3];
    "#);
    assert!(matches!(err, RuntimeError::IndexOutOfBounds { .. }));
}

// A `define` field on a thread template must NOT be accessible via the thread
// handle; only `expose` / `expose_mutable` are external. Anchors R-DEFINE-1 / P0-5.
#[tokio::test]
async fn test_define_field_external_invisible() {
    let err = assert_runs_err!(r#"
        $template W() {
            define secret: int = 42;
            async function __on_terminate__(): void {}
        }
        $W() { await keepalive(); } := h;
        let v: int = h.secret;
        h.terminate().wait();
    "#);
    let msg = err.to_string();
    assert!(
        msg.contains("no field 'secret' on thread handle"),
        "expected 'no field' panic for define access, got: {msg}",
    );
}

// Tessera's `int` is 32-bit signed; a literal that does not round-trip through
// i32 must fail at parse time rather than silently truncate in eval (P0-4).
#[tokio::test]
async fn test_int_literal_overflow() {
    let tokens = tessera_lexer::lex("let x: int = 9999999999;");
    let (_program, errors) = tessera_parser::parse(tokens);
    assert!(
        errors.iter().any(|e| e.message.contains("does not fit in `int`")),
        "expected int-overflow parse error, got: {:?}",
        errors,
    );
}

// After terminate().wait() resolves, dispatching a handler on the dead thread
// must surface the failure with kind == "TargetTerminated" (not the prior
// implementation's "TargetGone"). Anchors the A-1 / P0-1 fix.
#[tokio::test]
async fn test_target_terminated_kind() {
    assert_runs_ok!(r#"
        $template W() {
            async function __on_terminate__(): void {}
            async handler ping_me(): String { return "ok"; }
        }
        $W() { await keepalive(); } := h;
        h.terminate().wait();
        let r: Result<String, error> = try await h.ping_me();
        assert(r.isErr());
        let e: error = r.unwrapErr();
        assert(e.kind == "TargetTerminated");
    "#);
}
