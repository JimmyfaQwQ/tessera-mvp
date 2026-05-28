//! Runtime scope chain for interpreter evaluation.
//!
//! This is the value-typed (`Value`) sibling of `tessera_types::env::TypeEnv`,
//! which is the type-typed scope chain used during type checking. The two are
//! intentionally not unified: they serve different compilation phases and have
//! no shared lookup logic — sharing would force one phase to depend on the
//! other's value type. If a third phase (e.g. a constant folder) ever needs
//! the same shape, revisit this decision.

use std::collections::HashMap;
use tessera_runtime::Value;

#[derive(Debug, Default)]
pub struct Env {
    scopes: Vec<HashMap<String, Value>>,
}

impl Env {
    pub fn new() -> Self {
        Self { scopes: vec![HashMap::new()] }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub fn define(&mut self, name: String, value: Value) {
        self.scopes.last_mut().unwrap().insert(name, value);
    }

    pub fn lookup(&self, name: &str) -> Option<&Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) { return Some(v); }
        }
        None
    }

    pub fn assign(&mut self, name: &str, value: Value) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return true;
            }
        }
        false
    }
}
