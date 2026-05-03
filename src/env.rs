use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::value::Value;

#[derive(Debug, Clone)]
pub struct Env {
    vars: HashMap<String, Value>,
    parent: Option<Rc<RefCell<Env>>>,
}

impl Env {
    pub fn new() -> Self {
        Env {
            vars: HashMap::new(),
            parent: None,
        }
    }

    pub fn child(parent: Rc<RefCell<Env>>) -> Self {
        Env {
            vars: HashMap::new(),
            parent: Some(parent),
        }
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.vars.get(name) {
            return Some(v.clone());
        }
        self.parent.as_ref()?.borrow().get(name)
    }

    /// Set a variable in the nearest scope that already owns it, or create in current scope.
    pub fn set(&mut self, name: &str, val: Value) {
        if self.vars.contains_key(name) {
            self.vars.insert(name.to_string(), val);
            return;
        }
        if let Some(parent) = &self.parent {
            // Extract the boolean before the borrow is dropped to avoid RefCell double-borrow panic.
            let has = parent.borrow().has(name);
            if has {
                parent.borrow_mut().set(name, val);
                return;
            }
        }
        self.vars.insert(name.to_string(), val);
    }

    /// Define a new variable in the current scope (for let bindings).
    pub fn define(&mut self, name: &str, val: Value) {
        self.vars.insert(name.to_string(), val);
    }

    pub fn has(&self, name: &str) -> bool {
        if self.vars.contains_key(name) {
            return true;
        }
        self.parent.as_ref().is_some_and(|p| p.borrow().has(name))
    }

    pub fn all_names(&self) -> Vec<String> {
        self.vars.keys().cloned().collect()
    }

    pub fn vars(&self) -> HashMap<String, Value> {
        self.vars.clone()
    }
}

impl Default for Env {
    fn default() -> Self {
        Env::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_get_set() {
        let mut env = Env::new();
        env.define("x", Value::Int(10));
        assert_eq!(env.get("x").unwrap().to_string(), "10");
    }

    #[test]
    fn child_scope_sees_parent() {
        let parent = Rc::new(RefCell::new(Env::new()));
        parent.borrow_mut().define("x", Value::Int(42));
        let child = Env::child(Rc::clone(&parent));
        assert_eq!(child.get("x").unwrap().to_string(), "42");
    }

    #[test]
    fn child_shadows_parent() {
        let parent = Rc::new(RefCell::new(Env::new()));
        parent.borrow_mut().define("x", Value::Int(1));
        let mut child = Env::child(Rc::clone(&parent));
        child.define("x", Value::Int(2));
        assert_eq!(child.get("x").unwrap().to_string(), "2");
        assert_eq!(parent.borrow().get("x").unwrap().to_string(), "1");
    }

    #[test]
    fn set_mutates_existing() {
        let parent = Rc::new(RefCell::new(Env::new()));
        parent.borrow_mut().define("x", Value::Int(1));
        let mut child = Env::child(Rc::clone(&parent));
        child.set("x", Value::Int(99));
        // parent should be updated since child doesn't own x
        assert_eq!(parent.borrow().get("x").unwrap().to_string(), "99");
    }
}
