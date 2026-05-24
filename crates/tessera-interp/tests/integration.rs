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
