use super::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A lexical scope environment with optional parent for scope chaining.
/// In hack mode (pedantic=0), all values are reference-counted — no ownership tracking.
#[derive(Debug)]
pub struct Environment {
    bindings: HashMap<String, Value>,
    parent: Option<Rc<RefCell<Environment>>>,
}

impl Environment {
    /// Create a new top-level environment.
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            parent: None,
        }
    }

    /// Create a child environment with a parent scope.
    pub fn with_parent(parent: Rc<RefCell<Environment>>) -> Self {
        Self {
            bindings: HashMap::new(),
            parent: Some(parent),
        }
    }

    /// Define a new binding in the current scope.
    pub fn set(&mut self, name: String, value: Value) {
        self.bindings.insert(name, value);
    }

    /// Look up a binding, walking up the scope chain.
    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(val) = self.bindings.get(name) {
            Some(val.clone())
        } else if let Some(parent) = &self.parent {
            parent.borrow().get(name)
        } else {
            None
        }
    }

    /// Update an existing binding anywhere in the scope chain.
    pub fn update(&mut self, name: &str, value: Value) -> Result<(), ()> {
        if self.bindings.contains_key(name) {
            self.bindings.insert(name.to_string(), value);
            Ok(())
        } else if let Some(parent) = &self.parent {
            parent.borrow_mut().update(name, value)
        } else {
            Err(())
        }
    }
}
