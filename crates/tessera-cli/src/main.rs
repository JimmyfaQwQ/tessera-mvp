use std::{env, fs, process};

use tessera_lexer::lex;
use tessera_parser::parse;
use tessera_types::{TypeEnv, TypeChecker};
use tessera_lint::LintRunner;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let (path, check_only, dump_tokens, dump_ast) = parse_args(&args);

    let source = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => { eprintln!("error reading '{}': {}", path, e); process::exit(1); }
    };

    // Lex
    let tokens = lex(&source);
    if dump_tokens {
        for tok in &tokens {
            println!("{:?}  {:?}", tok.node, tok.span);
        }
        return;
    }

    // Parse
    let (program, parse_errors) = parse(tokens);
    if !parse_errors.is_empty() {
        for e in &parse_errors {
            eprintln!("[parse] {}", e);
        }
        process::exit(1);
    }
    if dump_ast {
        println!("{:#?}", program);
        return;
    }

    // Type check
    let mut env = TypeEnv::new();
    let mut checker = TypeChecker::new(&mut env);
    checker.check_program(&program);
    if !env.diagnostics.is_empty() {
        for d in &env.diagnostics {
            eprintln!("[type] {}", d.message);
        }
        process::exit(1);
    }

    // Lint
    let mut runner = LintRunner::default_passes();
    let lint_diags = runner.run_all(&program, &env);
    if !lint_diags.is_empty() {
        for d in &lint_diags {
            let level = match d.severity {
                tessera_lint::Severity::Error => "error",
                tessera_lint::Severity::Warn  => "warn",
                tessera_lint::Severity::Info  => "info",
            };
            eprintln!("[{}][{}] {}", level, d.rule_id, d.message);
            if let Some(help) = &d.help { eprintln!("  help: {}", help); }
        }
        let has_error = lint_diags.iter().any(|d| d.severity == tessera_lint::Severity::Error);
        if has_error { process::exit(1); }
    }

    if check_only { return; }

    // Run
    if let Err(e) = tessera_interp::run(&program).await {
        eprintln!("[runtime] thread crashed: {}", e);
        process::exit(1);
    }
}

fn parse_args(args: &[String]) -> (String, bool, bool, bool) {
    let mut check_only  = false;
    let mut dump_tokens = false;
    let mut dump_ast    = false;
    let mut path        = String::new();

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "--check"       => check_only  = true,
            "--dump-tokens" => dump_tokens = true,
            "--dump-ast"    => dump_ast    = true,
            _ if path.is_empty() => path = arg.clone(),
            _ => eprintln!("unknown argument: {}", arg),
        }
    }
    if path.is_empty() {
        eprintln!("usage: tessera [--check|--dump-tokens|--dump-ast] <file.tss>");
        process::exit(1);
    }
    (path, check_only, dump_tokens, dump_ast)
}
