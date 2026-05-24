use std::{env, fs, process};

use tessera_lexer::{lex, lex_errors};
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

    // Surface any invalid characters the lexer could not recognise.
    let lex_errs = lex_errors(&source, &tokens);
    if !lex_errs.is_empty() {
        for e in lex_errs {
            emit(e, &path, &source);
        }
        process::exit(1);
    }

    // Parse
    let (program, parse_errors) = parse(tokens);
    if !parse_errors.is_empty() {
        // Error recovery can cascade, so cap the noise like other modern compilers.
        const MAX_PARSE_ERRORS: usize = 10;
        let total = parse_errors.len();
        for e in parse_errors.into_iter().take(MAX_PARSE_ERRORS) {
            emit(e, &path, &source);
        }
        if total > MAX_PARSE_ERRORS {
            eprintln!("... and {} more parse error(s) suppressed", total - MAX_PARSE_ERRORS);
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
            emit(d.clone(), &path, &source);
        }
        process::exit(1);
    }

    // Lint
    let mut runner = LintRunner::default_passes();
    let lint_diags = runner.run_all(&program, &env);
    if !lint_diags.is_empty() {
        for d in &lint_diags {
            emit(d.clone(), &path, &source);
        }
        let has_error = lint_diags.iter().any(|d| d.severity == tessera_lint::Severity::Error);
        if has_error { process::exit(1); }
    }

    if check_only { return; }

    // Run
    if let Err(report) = tessera_interp::run_reported(&program).await {
        emit(report.error.clone(), &path, &source);
        if !report.backtrace.is_empty() {
            eprintln!("\ntraceback (most recent call last):");
            for frame in &report.backtrace {
                let (line, col) = line_col(&source, frame.span.start);
                eprintln!("    at {} ({}:{}:{})", frame.name, path, line, col);
            }
        }
        process::exit(1);
    }
}

/// Render any miette diagnostic to stderr with source context (line preview,
/// caret underline, help, error code) via miette's graphical handler.
fn emit<D>(diag: D, path: &str, source: &str)
where
    D: miette::Diagnostic + Send + Sync + 'static,
{
    let report = miette::Report::new(diag)
        .with_source_code(miette::NamedSource::new(path, source.to_string()));
    eprintln!("{report:?}");
}

/// Convert a byte offset into 1-based line and column numbers.
fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, ch) in src.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
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
