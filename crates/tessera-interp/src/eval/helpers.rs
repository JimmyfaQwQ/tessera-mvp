//! Pure helper functions used by interpreter sites without needing access to
//! `&Interpreter`. Kept here so the main `Interpreter` impl in `mod.rs` and
//! `builtin.rs` reads as control flow over value transforms.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use tessera_ast::*;
use tessera_runtime::{BreakablePrimitive, RuntimeError, Value};

// ── Structured-error helpers ──────────────────────────────────────────────────

/// Convert any `RuntimeError` into a `Value::ErrorObj` for use in `try` results.
/// The variant→(kind, message) mapping lives in `RuntimeError::kind_and_message`
/// so adding a new variant is a single-file change there.
pub(super) fn runtime_error_to_error_obj(e: &RuntimeError) -> Value {
    let (kind, message) = e.kind_and_message();
    Value::ErrorObj { kind, message }
}

/// Collect Arc<dyn BreakablePrimitive> for all define fields in `self_map` that
/// hold a Signal, Contract, or Permit. Used for scope binding in `exec_scope_block`.
pub(super) fn collect_define_field_prims(
    self_map: &Rc<RefCell<HashMap<String, Value>>>,
    define_names: &[String],
) -> Vec<Arc<dyn BreakablePrimitive>> {
    let map = self_map.borrow();
    let mut prims: Vec<Arc<dyn BreakablePrimitive>> = Vec::new();
    for name in define_names {
        if let Some(val) = map.get(name) {
            match val {
                Value::Signal(s)   => prims.push(Arc::clone(s) as Arc<dyn BreakablePrimitive>),
                Value::Contract(c) => prims.push(Arc::clone(c) as Arc<dyn BreakablePrimitive>),
                Value::Permit(p)   => prims.push(Arc::clone(p) as Arc<dyn BreakablePrimitive>),
                _ => {}
            }
        }
    }
    prims
}

// ── Scope / thread template hook lookups ──────────────────────────────────────

pub(super) fn find_scope_hook<'a>(decl: &'a ScopeTemplateDecl, name: &str) -> Option<&'a FuncDef> {
    for m in &decl.members {
        match m {
            ScopeTemplateMember::OnEnter(f) if name == "__on_enter__" => return Some(f),
            ScopeTemplateMember::OnExit(f)  if name == "__on_exit__"  => return Some(f),
            _ => {}
        }
    }
    None
}

pub fn find_thread_hook<'a>(decl: &'a ThreadTemplateDecl, name: &str) -> Option<&'a FuncDef> {
    for m in &decl.members {
        match m {
            ThreadTemplateMember::OnEnter(f)    if name == "__on_enter__"    => return Some(f),
            ThreadTemplateMember::OnExit(f)     if name == "__on_exit__"     => return Some(f),
            ThreadTemplateMember::OnTerminate(f) if name == "__on_terminate__" => return Some(f),
            _ => {}
        }
    }
    None
}

pub fn find_handler<'a>(decl: &'a ThreadTemplateDecl, name: &str) -> Option<&'a HandlerDef> {
    for m in &decl.members {
        if let ThreadTemplateMember::Handler(h) = m {
            if h.name.name == name { return Some(h); }
        }
    }
    None
}

// ── Runtime argument type helpers ─────────────────────────────────────────────

/// Check if `val` is compatible with the declared parameter type `te`.
/// Unknown / unrecognised type annotations are skipped (returns true).
pub(super) fn runtime_type_matches(te: &tessera_ast::TypeExpr, val: &Value) -> bool {
    match te {
        tessera_ast::TypeExpr::Void  => matches!(val, Value::Void),
        tessera_ast::TypeExpr::Never => false,
        tessera_ast::TypeExpr::Named(ident, _) => {
            let expected = match ident.name.as_str() {
                "bool"    => "bool",
                "int"     => "int",
                "double"  => "double",
                "char"    => "char",
                "String"  => "String",
                "void"    => return matches!(val, Value::Void),
                "never"   => return false,
                "List"    => "List",
                "Map"     => "Map",
                "Option"  => "Option",
                "Result"  => "Result",
                "locked"  => "locked",
                "Queue"   => "Queue",
                "signal"  => "signal",
                "contract" => "contract",
                "permit"  => "permit",
                "Future"  => "Future",
                "HandlerFuture" => "HandlerFuture",
                "thread"  => "ThreadHandle",
                _ => return true,
            };
            val.type_name() == expected
        }
    }
}

pub(super) fn type_expr_display(te: &tessera_ast::TypeExpr) -> String {
    match te {
        tessera_ast::TypeExpr::Void => "void".to_string(),
        tessera_ast::TypeExpr::Never => "never".to_string(),
        tessera_ast::TypeExpr::Named(ident, args) if args.is_empty() => ident.name.clone(),
        tessera_ast::TypeExpr::Named(ident, args) => {
            let inner: Vec<String> = args.iter().map(type_expr_display).collect();
            format!("{}<{}>", ident.name, inner.join(", "))
        }
    }
}

// ── Literal / equality / truthiness ──────────────────────────────────────────

pub(super) fn eval_literal(l: &Literal) -> Value {
    match &l.kind {
        LitKind::Bool(b) => Value::Bool(*b),
        LitKind::Int(i)  => Value::Int(*i as i32),
        LitKind::Double(f) => Value::Double(*f),
        LitKind::Char(c)  => Value::Char(*c),
        LitKind::String(s) => Value::Str(s.clone()),
        LitKind::None => Value::Option(None),
    }
}

pub(super) fn truthy(v: Value) -> bool {
    match v {
        Value::Bool(b)       => b,
        Value::Int(i)        => i != 0,
        Value::Option(None)  => false,
        Value::Option(Some(_)) => true,
        _ => true,
    }
}

pub(super) fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Int(a),  Value::Int(b))  => a == b,
        (Value::Double(a), Value::Double(b)) => a == b,
        (Value::Char(a), Value::Char(b))  => a == b,
        (Value::Str(a),  Value::Str(b))   => a == b,
        (Value::Void,    Value::Void)     => true,
        (Value::Option(None), Value::Option(None)) => true,
        (Value::HandlerFuture(hf), Value::Result(Err(e))) |
        (Value::Result(Err(e)), Value::HandlerFuture(hf)) => {
            match hf.get_err() {
                Some(msg) => values_equal(&Value::Str(msg), e),
                None => false,
            }
        }
        (Value::Signal(s), Value::Result(Err(e))) |
        (Value::Result(Err(e)), Value::Signal(s)) => match s.broken_reason() {
            Some(r) => values_equal(&Value::Str(r.as_str().into()), e),
            None    => false,
        },
        (Value::Contract(c), Value::Result(Err(e))) |
        (Value::Result(Err(e)), Value::Contract(c)) => match c.broken_reason() {
            Some(r) => values_equal(&Value::Str(r.as_str().into()), e),
            None    => false,
        },
        (Value::Permit(p), Value::Result(Err(e))) |
        (Value::Result(Err(e)), Value::Permit(p)) => match p.broken_reason() {
            Some(r) => values_equal(&Value::Str(r.as_str().into()), e),
            None    => false,
        },
        _ => false,
    }
}

pub fn value_to_string(v: &Value) -> String {
    match v {
        Value::Bool(b)   => b.to_string(),
        Value::Int(i)    => i.to_string(),
        Value::Double(f) => f.to_string(),
        Value::Char(c)   => c.to_string(),
        Value::Str(s)    => s.clone(),
        Value::Void      => "void".into(),
        Value::Never     => "never".into(),
        Value::Option(None)    => "None".into(),
        Value::Option(Some(v)) => format!("Some({})", value_to_string(v)),
        Value::Result(Ok(v))   => format!("Ok({})", value_to_string(v)),
        Value::Result(Err(e))  => format!("Err({})", value_to_string(e)),
        Value::ErrorObj { kind, message } => format!("error({kind}: {message})"),
        _ => "<complex>".into(),
    }
}
