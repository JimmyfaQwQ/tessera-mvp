//! Integration tests for `tessera-types` diagnostics that are not exercised by
//! the interpreter path (the interpreter does not run the type checker).

use tessera_lexer::lex;
use tessera_parser::parse;
use tessera_types::{TypeChecker, TypeEnv};

/// Lex → parse → type-check, returning the type-checker diagnostic messages.
fn diagnostics(src: &str) -> Vec<String> {
    let tokens = lex(src);
    let (program, _parse_errors) = parse(tokens);
    let mut env = TypeEnv::new();
    TypeChecker::new(&mut env).check_program(&program);
    env.diagnostics.iter().map(|d| d.message.clone()).collect()
}

fn has_dup(diags: &[String]) -> bool {
    diags.iter().any(|m| m.contains("already declared in this block"))
}

// ── §3.3: same-block re-declaration is rejected ──────────────────────────────

#[test]
fn duplicate_let_in_same_block_is_rejected() {
    let diags = diagnostics(r#"
        let x: int = 1;
        let x: int = 2;
    "#);
    assert!(has_dup(&diags), "expected duplicate-declaration error, got: {diags:?}");
}

#[test]
fn duplicate_let_in_template_member_body_is_rejected() {
    let diags = diagnostics(r#"
        $template W() {
            async function __on_terminate__(): void {}
            function run(): void {
                let y: int = 1;
                let y: int = 2;
            }
        }
    "#);
    assert!(has_dup(&diags), "expected duplicate-declaration error, got: {diags:?}");
}

// ── shadowing in a *nested* block remains legal ──────────────────────────────

#[test]
fn shadowing_in_nested_block_is_ok() {
    let diags = diagnostics(r#"
        let x: int = 1;
        while (true) {
            let x: int = 2;
            break;
        }
    "#);
    assert!(!has_dup(&diags), "nested-block shadowing must be allowed, got: {diags:?}");
}

#[test]
fn same_name_in_separate_sibling_blocks_is_ok() {
    let diags = diagnostics(r#"
        for (let i: int = 0; i < 1; i = i + 1) { let t: int = i; }
        for (let i: int = 0; i < 1; i = i + 1) { let t: int = i; }
    "#);
    assert!(!has_dup(&diags), "separate sibling blocks must be allowed, got: {diags:?}");
}

// ── a `let` re-using a parameter name (different scope) is shadowing, not a dup ─

#[test]
fn let_shadowing_param_is_ok() {
    let diags = diagnostics(r#"
        function f(n: int): void {
            let n: int = n + 1;
        }
    "#);
    assert!(!has_dup(&diags), "let shadowing a param must be allowed, got: {diags:?}");
}
