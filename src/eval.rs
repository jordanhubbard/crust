use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;

use crate::ast::*;
use crate::env::Env;
use crate::error::CrustError;
use crate::value::{CrustFn, Value};

// ── Control flow signal ───────────────────────────────────────────────────────

pub enum Signal {
    Return(Value),
    Break(Option<Value>),
    Continue,
    Err(CrustError),
}

impl From<CrustError> for Signal {
    fn from(e: CrustError) -> Self { Signal::Err(e) }
}

type EvalResult = Result<Value, Signal>;

fn err(msg: impl Into<String>) -> Signal {
    Signal::Err(CrustError::runtime(msg))
}

// ── Pattern binding ───────────────────────────────────────────────────────────

fn bind_pat(pat: &Pat, val: Value, env: &Rc<RefCell<Env>>) {
    match pat {
        Pat::Ident(name) => env.borrow_mut().define(name, val),
        Pat::Wild => {}
        Pat::Tuple(pats) => {
            if let Value::Tuple(items) = val {
                for (p, v) in pats.iter().zip(items.into_iter()) {
                    bind_pat(p, v, env);
                }
            } else if let Value::Vec(items) = val {
                for (p, v) in pats.iter().zip(items.into_iter()) {
                    bind_pat(p, v, env);
                }
            }
        }
        Pat::Ref(inner) => bind_pat(inner, val, env),
        Pat::TupleStruct { fields, .. } => {
            if let Value::Tuple(items) = val {
                for (p, v) in fields.iter().zip(items.into_iter()) {
                    bind_pat(p, v, env);
                }
            }
        }
        Pat::Struct { fields, .. } => {
            if let Value::Struct { fields: sfields, .. } = val {
                for (fname, fpat) in fields {
                    if let Some(fval) = sfields.get(fname).cloned() {
                        bind_pat(fpat, fval, env);
                    }
                }
            }
        }
        Pat::Bind { name, pat } => {
            env.borrow_mut().define(name, val.clone());
            bind_pat(pat, val, env);
        }
        _ => {}
    }
}

// Coerce a value to match a declared type annotation (Vec↔HashMap conversions only)
fn coerce_by_ty(val: Value, ty: Option<&Ty>) -> Value {
    match ty {
        Some(Ty::Named(name)) if name == "String" || name == "str" => {
            // String/str annotation: if we got a Vec<char>, join into String
            if let Value::Vec(ref v) = val {
                if v.iter().all(|x| matches!(x, Value::Char(_))) {
                    if let Value::Vec(v) = val {
                        let s: String = v.iter().filter_map(|c| if let Value::Char(ch) = c { Some(*ch) } else { None }).collect();
                        return Value::Str(s);
                    }
                }
            }
            val
        }
        Some(Ty::Generic(name, _)) if name == "Vec" => {
            // Vec annotation: if we got a HashMap, convert to Vec<(k, v)>
            if let Value::HashMap(m) = val {
                let mut pairs: Vec<Value> = m.into_iter()
                    .map(|(k, v)| {
                        // Try to restore numeric keys
                        let kv = if let Ok(n) = k.parse::<i64>() { Value::Int(n) } else { Value::Str(k) };
                        Value::Tuple(vec![kv, v])
                    })
                    .collect();
                pairs.sort_by_key(|p| if let Value::Tuple(t) = p { t[0].to_string() } else { String::new() });
                return Value::Vec(pairs);
            }
            val
        }
        Some(Ty::Generic(name, _)) if name == "HashMap" => {
            // HashMap annotation: if we got a Vec of 2-tuples, convert to HashMap
            vec_tuples_to_hashmap(val)
        }
        Some(Ty::Named(name)) if name == "HashMap" => {
            vec_tuples_to_hashmap(val)
        }
        _ => val,
    }
}

fn vec_tuples_to_hashmap(val: Value) -> Value {
    if let Value::Vec(ref v) = val {
        if !v.is_empty() && v.iter().all(|x| matches!(x, Value::Tuple(t) if t.len() == 2)) {
            if let Value::Vec(v) = val {
                let mut m = std::collections::HashMap::new();
                for item in v {
                    if let Value::Tuple(mut t) = item {
                        let v2 = t.pop().unwrap();
                        let k = t.pop().unwrap().to_string();
                        m.insert(k, v2);
                    }
                }
                return Value::HashMap(m);
            }
        }
    }
    val
}

// ── EntryRef helpers ──────────────────────────────────────────────────────────
// map_name is either a plain env var name, or "__sf__::struct_var::field_name"
// for HashMap values stored inside struct fields.

fn lookup_entry_map(map_name: &str, env: &Rc<RefCell<Env>>) -> Option<Value> {
    if let Some(path) = map_name.strip_prefix("__sf__::") {
        let mut parts = path.splitn(2, "::");
        let sv = parts.next()?;
        let fn_ = parts.next()?;
        match env.borrow().get(sv)? {
            Value::Struct { fields, .. } => fields.get(fn_).cloned(),
            _ => None,
        }
    } else {
        env.borrow().get(map_name)
    }
}

fn write_back_entry_map(map_name: &str, new_val: Value, env: &Rc<RefCell<Env>>) {
    if let Some(path) = map_name.strip_prefix("__sf__::") {
        let mut parts = path.splitn(2, "::");
        if let (Some(sv), Some(fn_)) = (parts.next(), parts.next()) {
            let struct_opt = env.borrow().get(sv); // Ref dropped before if-let body
            if let Some(Value::Struct { type_name, mut fields }) = struct_opt {
                fields.insert(fn_.to_string(), new_val);
                env.borrow_mut().set(sv, Value::Struct { type_name, fields });
            }
        }
    } else {
        env.borrow_mut().set(map_name, new_val);
    }
}

// ── Interpreter ───────────────────────────────────────────────────────────────

pub struct Interpreter {
    fns: HashMap<String, CrustFn>,
    structs: HashMap<String, StructDef>,
    impls: HashMap<String, Vec<FnDef>>,
    traits: HashMap<String, Vec<FnDef>>, // default trait methods
    pub output: Vec<String>,
    // Populated by call_crust_fn when a &mut self method modifies self
    pub self_writeback: Option<Value>,
    // Populated by call_crust_fn for &mut params: (param_index, new_value)
    pub mut_writebacks: Vec<(usize, Value)>,
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            fns: HashMap::new(),
            structs: HashMap::new(),
            impls: HashMap::new(),
            traits: HashMap::new(),
            output: Vec::new(),
            self_writeback: None,
            mut_writebacks: Vec::new(),
        }
    }

    pub fn run(&mut self, program: Program) -> Result<(), CrustError> {
        // First pass: register all top-level items
        for item in &program {
            self.register_item(item.clone())?;
        }
        // Second pass: call main()
        let env = Rc::new(RefCell::new(Env::new()));
        match self.call_fn("main", vec![], Rc::clone(&env)) {
            Ok(_) => Ok(()),
            Err(Signal::Err(e)) => Err(e),
            Err(Signal::Return(_)) => Ok(()),
            Err(_) => Ok(()),
        }
    }

    pub fn register_item_pub(&mut self, item: Item) -> Result<(), CrustError> {
        self.register_item(item)
    }

    pub fn eval_stmt_pub(&mut self, stmt: &Stmt, env: Rc<RefCell<Env>>) -> EvalResult {
        self.eval_stmt(stmt, env)
    }

    pub fn fn_names(&self) -> Vec<String> {
        self.fns.keys().cloned().collect()
    }

    fn register_item(&mut self, item: Item) -> Result<(), CrustError> {
        match item {
            Item::Fn(def) => {
                self.fns.insert(def.name.clone(), CrustFn { params: def.params, ret_ty: def.ret_ty, body: def.body, captured: None });
            }
            Item::Struct(def) => { self.structs.insert(def.name.clone(), def); }
            Item::Enum(_) => {} // enums are constructed by name at runtime
            Item::Trait { name, methods } => {
                self.traits.insert(name, methods);
            }
            Item::Impl(def) => {
                // If this is `impl TraitName for TypeName`, inject default trait methods
                // for any method not overridden in this impl block.
                if let Some(trait_name) = &def.trait_name {
                    let defaults = self.traits.get(trait_name).cloned().unwrap_or_default();
                    let override_names: std::collections::HashSet<&str> =
                        def.methods.iter().map(|m| m.name.as_str()).collect();
                    let to_add: Vec<FnDef> = defaults.into_iter()
                        .filter(|m| !override_names.contains(m.name.as_str()))
                        .collect();
                    self.impls.entry(def.type_name.clone()).or_default().extend(to_add);
                }
                self.impls.entry(def.type_name.clone()).or_default().extend(def.methods);
            }
            Item::Use(_) | Item::Const { .. } | Item::TypeAlias { .. } => {}
        }
        Ok(())
    }

    // ── Function calls ────────────────────────────────────────────────────────

    fn call_fn(&mut self, name: &str, args: Vec<Value>, env: Rc<RefCell<Env>>) -> EvalResult {
        // Check local env first (closures/fn-params bound by name).
        // Extract the value in a separate statement so the borrow is dropped before
        // call_crust_fn runs (which may need to borrow_mut the same env via captures).
        let local_fn = env.borrow().get(name);
        if let Some(Value::Fn(cfn)) = local_fn {
            return self.call_crust_fn(&cfn, args, None);
        }

        // Check top-level registered functions
        let func = self.fns.get(name).cloned();
        if let Some(cfn) = func {
            return self.call_crust_fn(&cfn, args, None);
        }

        // Built-in free functions
        crate::stdlib::call_builtin(name, args, self)
            .ok_or_else(|| err(format!("undefined function: {}", name)))
            .and_then(|r| r)
    }

    pub fn call_crust_fn(&mut self, cfn: &CrustFn, args: Vec<Value>, self_val: Option<Value>) -> EvalResult {
        let base = cfn.captured.clone().unwrap_or_else(|| Rc::new(RefCell::new(Env::new())));
        let child = Rc::new(RefCell::new(Env::child(base)));
        let has_self = self_val.is_some();

        // Bind parameters
        let mut arg_iter = args.into_iter();
        for param in &cfn.params {
            if param.is_self {
                if let Some(sv) = &self_val {
                    child.borrow_mut().define("self", sv.clone());
                }
            } else {
                let val = arg_iter.next().unwrap_or(Value::Unit);
                child.borrow_mut().define(&param.name, val);
            }
        }

        let result = match self.eval_block(&cfn.body, Rc::clone(&child)) {
            Ok(v) => Ok(coerce_by_ty(v, cfn.ret_ty.as_ref())),
            Err(Signal::Return(v)) => Ok(coerce_by_ty(v, cfn.ret_ty.as_ref())),
            Err(e) => Err(e),
        };

        // Capture modified self for &mut self methods
        if has_self {
            self.self_writeback = child.borrow().get("self");
        }

        // Capture modified &mut params for writeback
        self.mut_writebacks.clear();
        let mut arg_idx = 0usize;
        for param in &cfn.params {
            if param.is_self { continue; }
            if matches!(param.ty, Ty::Ref(true, _)) {
                if let Some(v) = child.borrow().get(&param.name) {
                    self.mut_writebacks.push((arg_idx, v));
                }
            }
            arg_idx += 1;
        }

        result
    }

    // ── Block evaluation ──────────────────────────────────────────────────────

    fn eval_block(&mut self, block: &Block, env: Rc<RefCell<Env>>) -> EvalResult {
        // Register local item definitions first
        for stmt in &block.stmts {
            if let Stmt::Item(item) = stmt {
                self.register_item(item.clone()).map_err(Signal::Err)?;
            }
        }

        let mut last = Value::Unit;
        for stmt in &block.stmts {
            last = self.eval_stmt(stmt, Rc::clone(&env))?;
        }
        if let Some(tail) = &block.tail {
            Ok(self.eval_expr(tail, Rc::clone(&env))?)
        } else {
            Ok(last)
        }
    }

    // ── Statement evaluation ──────────────────────────────────────────────────

    fn eval_stmt(&mut self, stmt: &Stmt, env: Rc<RefCell<Env>>) -> EvalResult {
        match stmt {
            Stmt::Let { name, ty, init, .. } => {
                let val = if let Some(expr) = init {
                    self.eval_expr(expr, Rc::clone(&env))?
                } else {
                    Value::Unit
                };
                // Coerce between Vec and HashMap based on type annotation
                let val = coerce_by_ty(val, ty.as_ref());
                env.borrow_mut().define(name, val);
                Ok(Value::Unit)
            }
            Stmt::LetPat { pat, init, .. } => {
                let val = if let Some(expr) = init {
                    self.eval_expr(expr, Rc::clone(&env))?
                } else {
                    Value::Unit
                };
                bind_pat(pat, val, &env);
                Ok(Value::Unit)
            }
            Stmt::Semi(expr) => {
                self.eval_expr(expr, env)?;
                Ok(Value::Unit)
            }
            Stmt::Expr(expr) => {
                self.eval_expr(expr, env)
            }
            Stmt::Item(item) => {
                // already registered in eval_block's first pass
                let _ = item;
                Ok(Value::Unit)
            }
        }
    }

    // ── Expression evaluation ─────────────────────────────────────────────────

    pub fn eval_expr(&mut self, expr: &Expr, env: Rc<RefCell<Env>>) -> EvalResult {
        match expr {
            Expr::Lit(lit) => Ok(eval_lit(lit)),

            Expr::Ident(name) => {
                if let Some(v) = env.borrow().get(name) {
                    // Auto-deref EntryRef when used in value context
                    if let Value::EntryRef { ref map_name, ref key } = v {
                        let map_name = map_name.clone();
                        let key = key.clone();
                        let map_val = lookup_entry_map(&map_name, &env)
                            .ok_or_else(|| err(format!("no map `{}`", map_name)))?;
                        if let Value::HashMap(m) = map_val {
                            return Ok(m.get(&key).cloned().unwrap_or(Value::Unit));
                        }
                    }
                    return Ok(v);
                }
                // Fall back to top-level function as a value
                if let Some(cfn) = self.fns.get(name).cloned() {
                    return Ok(Value::Fn(cfn));
                }
                // Fall back: zero-arg builtins like `None`
                if let Some(r) = crate::stdlib::call_builtin(name, vec![], self) {
                    return r;
                }
                let hint = if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                    " (if this is an enum variant, make sure it's constructed with `Type::Variant` or `Variant(args)`)"
                } else { "" };
                Err(err(format!("undefined variable: `{}`{}", name, hint)))
            }

            Expr::Path(parts) => {
                // Handle None, Some, Ok, Err, enum variants, and constants like i64::MAX
                match parts.as_slice() {
                    [only] => env.borrow().get(only)
                        .ok_or_else(|| err(format!("undefined: {}", only))),
                    _ => {
                        let name = parts.last().unwrap().clone();
                        let type_name = parts[parts.len()-2].clone();
                        // Try full path and progressively shorter paths as qualified lookups
                        // std::f64::consts::PI → try "std::f64::consts::PI", "f64::consts::PI", "consts::PI"
                        let full = parts.join("::");
                        if let Some(r) = crate::stdlib::call_builtin(&full, vec![], self) {
                            return r;
                        }
                        let qualified = format!("{}::{}", type_name, name);
                        if let Some(r) = crate::stdlib::call_builtin(&qualified, vec![], self) {
                            return r;
                        }
                        // Try as zero-arg method (may cover more constants)
                        if let Ok(v) = self.call_method_or_static(&type_name, &name, None, vec![], Rc::clone(&env)) {
                            return Ok(v);
                        }
                        // Fall back: enum variant constructor
                        let type_name = parts[..parts.len()-1].join("::");
                        Ok(Value::Enum { type_name, variant: name, inner: None })
                    }
                }
            }

            Expr::Unary(op, inner) => {
                let val = self.eval_expr(inner, env)?;
                eval_unary(op, val)
            }

            Expr::Binary(op, lhs, rhs) => {
                // Short-circuit for && and ||
                match op {
                    BinOp::And => {
                        let l = self.eval_expr(lhs, Rc::clone(&env))?;
                        if !l.is_truthy() { return Ok(Value::Bool(false)); }
                        let r = self.eval_expr(rhs, env)?;
                        return Ok(Value::Bool(r.is_truthy()));
                    }
                    BinOp::Or => {
                        let l = self.eval_expr(lhs, Rc::clone(&env))?;
                        if l.is_truthy() { return Ok(Value::Bool(true)); }
                        let r = self.eval_expr(rhs, env)?;
                        return Ok(Value::Bool(r.is_truthy()));
                    }
                    _ => {}
                }
                let l = self.eval_expr(lhs, Rc::clone(&env))?;
                let r = self.eval_expr(rhs, env)?;
                eval_binary(op, l, r)
            }

            Expr::Assign(target, rhs_expr) => {
                let val = self.eval_expr(rhs_expr, Rc::clone(&env))?;
                self.assign(target, val, env)?;
                Ok(Value::Unit)
            }

            Expr::OpAssign(op, target, rhs_expr) => {
                let rval = self.eval_expr(rhs_expr, Rc::clone(&env))?;
                let lval = self.eval_expr(target, Rc::clone(&env))?;
                let result = eval_binary(op, lval, rval)?;
                self.assign(target, result, env)?;
                Ok(Value::Unit)
            }

            Expr::Call { func, args } => {
                let arg_vals: Vec<Value> = args.iter()
                    .map(|a| self.eval_expr(a, Rc::clone(&env)))
                    .collect::<Result<_, _>>()?;

                let result = match func.as_ref() {
                    Expr::Ident(name) => self.call_fn(name, arg_vals, Rc::clone(&env)),
                    Expr::Path(parts) => {
                        let fn_name = parts.last().unwrap().clone();
                        if parts.len() > 1 {
                            // Use only the immediate type name, stripping namespace prefixes.
                            // std::collections::HashMap::new → type="HashMap", fn="new"
                            let type_name = parts[parts.len()-2].clone();
                            self.call_method_or_static(&type_name, &fn_name, None, arg_vals, Rc::clone(&env))
                        } else {
                            self.call_fn(&fn_name, arg_vals, Rc::clone(&env))
                        }
                    }
                    _ => {
                        let func_val = self.eval_expr(func, Rc::clone(&env))?;
                        match func_val {
                            Value::Fn(cfn) => self.call_crust_fn(&cfn, arg_vals, None),
                            _ => Err(err(format!("not a function: {}", func_val.type_name()))),
                        }
                    }
                };
                // Write back &mut params
                let writebacks = std::mem::take(&mut self.mut_writebacks);
                for (idx, new_val) in writebacks {
                    apply_mut_writeback(args.get(idx), new_val, &env);
                }
                result
            }

            Expr::MethodCall { receiver, method, turbofish, args } => {
                // Remap collect based on turbofish: collect::<String>() → "collect_string"
                let method_str: String = if method == "collect" {
                    match turbofish.as_deref() {
                        Some("String") => "collect_string".to_string(),
                        _ => method.clone(),
                    }
                } else { method.clone() };
                let method: &str = &method_str;
                // Special-case: map.entry(k).or_insert(v) / or_insert_with(f)
                // map can be a plain ident or a struct field (self.field)
                if matches!(method, "or_insert" | "or_insert_with" | "or_default") {
                    if let Expr::MethodCall { receiver: map_expr, method: entry_m, args: entry_args, .. } = receiver.as_ref() {
                        if entry_m == "entry" {
                            let map_key_opt: Option<String> = match map_expr.as_ref() {
                                Expr::Ident(n) => Some(n.clone()),
                                Expr::Field(se, fn_) => {
                                    if let Expr::Ident(sv) = se.as_ref() {
                                        Some(format!("__sf__::{}::{}", sv, fn_))
                                    } else { None }
                                }
                                _ => None,
                            };
                            if let Some(map_key) = map_key_opt {
                                let map_val = self.eval_expr(map_expr, Rc::clone(&env))?;
                                if let Value::HashMap(mut m) = map_val {
                                    let key = entry_args.iter().map(|a| self.eval_expr(a, Rc::clone(&env))).next()
                                        .transpose()?.map(|v| v.to_string()).unwrap_or_default();
                                    let default_val = if method == "or_default" {
                                        Value::Int(0)
                                    } else if let Some(arg) = args.first() {
                                        let v = self.eval_expr(arg, Rc::clone(&env))?;
                                        if let Value::Fn(cfn) = v {
                                            self.call_crust_fn(&cfn, vec![], None)?
                                        } else { v }
                                    } else { Value::Unit };
                                    m.entry(key.clone()).or_insert(default_val);
                                    write_back_entry_map(&map_key, Value::HashMap(m), &env);
                                    return Ok(Value::EntryRef { map_name: map_key, key });
                                }
                            }
                        }
                    }
                }
                let recv_val = self.eval_expr(receiver, Rc::clone(&env))?;
                let arg_vals: Vec<Value> = args.iter()
                    .map(|a| self.eval_expr(a, Rc::clone(&env)))
                    .collect::<Result<_, _>>()?;

                // Dispatch mutating methods on an EntryRef (e.g. map.entry(k).or_insert_with(Vec::new).push(v))
                if let Value::EntryRef { ref map_name, ref key } = recv_val {
                    let map_name = map_name.clone();
                    let key = key.clone();
                    let map_val = lookup_entry_map(&map_name, &env)
                        .ok_or_else(|| err(format!("no map for entry ref `{}`", map_name)))?;
                    if let Value::HashMap(mut m) = map_val {
                        let entry_val = m.get(&key).cloned().unwrap_or(Value::Unit);
                        if let Some((ret_val, new_entry)) = crate::stdlib::call_method_mut(entry_val, method, arg_vals.clone(), self) {
                            let ret_val = ret_val?;
                            m.insert(key, new_entry);
                            write_back_entry_map(&map_name, Value::HashMap(m), &env);
                            return Ok(ret_val);
                        }
                    }
                }

                let type_name = match &recv_val {
                    Value::Struct { type_name, .. } => type_name.clone(),
                    Value::Enum { type_name, .. } => type_name.clone(),
                    v => v.type_name().to_string(),
                };
                // For mutating methods on named variables, apply mutation and write back
                let recv_ident = if let Expr::Ident(n) = receiver.as_ref() { Some(n.clone()) }
                    else { None };

                // For mutating methods on struct fields (e.g. self.data.push(v)),
                // apply mutation and write the updated field back into the struct
                if recv_ident.is_none() {
                    if let Expr::Field(struct_expr, field_name) = receiver.as_ref() {
                        let struct_ident = match struct_expr.as_ref() { Expr::Ident(n) => Some(n.clone()), _ => None };
                        if let Some(struct_var) = struct_ident {
                            if let Some((ret_val, new_field)) = crate::stdlib::call_method_mut(
                                recv_val.clone(), method, arg_vals.clone(), self
                            ) {
                                let ret_val = ret_val?;
                                let struct_val = env.borrow().get(&struct_var);
                                if let Some(Value::Struct { type_name: sty, mut fields }) = struct_val {
                                    fields.insert(field_name.clone(), new_field);
                                    env.borrow_mut().set(&struct_var, Value::Struct { type_name: sty, fields });
                                }
                                return Ok(ret_val);
                            }
                        }
                    }
                }

                let recv_ident_clone = recv_ident.clone();

                if let Some(ref var) = recv_ident {
                    if let Some((ret_val, new_recv)) = crate::stdlib::call_method_mut(
                        recv_val.clone(), method, arg_vals.clone(), self
                    ) {
                        let ret_val = ret_val?;
                        env.borrow_mut().set(var, new_recv);
                        return Ok(ret_val);
                    }
                }

                self.self_writeback = None;
                let result = self.call_method_or_static(&type_name, method, Some(recv_val), arg_vals, Rc::clone(&env))?;
                // Write back self if a &mut self method mutated it
                if let Some(new_self) = self.self_writeback.take() {
                    if let Some(var) = recv_ident_clone {
                        env.borrow_mut().set(&var, new_self);
                    }
                }
                // Write back &mut params
                let writebacks = std::mem::take(&mut self.mut_writebacks);
                for (idx, new_val) in writebacks {
                    apply_mut_writeback(args.get(idx), new_val, &env);
                }
                Ok(result)
            }

            Expr::Field(base, field) => {
                let val = self.eval_expr(base, env)?;
                match val {
                    Value::Struct { fields, .. } => {
                        fields.get(field).cloned()
                            .ok_or_else(|| err(format!("no field `{}` on struct", field)))
                    }
                    Value::Tuple(items) => {
                        let idx: usize = field.parse().map_err(|_| err(format!("invalid tuple index: {}", field)))?;
                        items.get(idx).cloned()
                            .ok_or_else(|| err(format!("tuple index {} out of bounds", idx)))
                    }
                    other => Err(err(format!("cannot access field `{}` on {}", field, other.type_name()))),
                }
            }

            Expr::Index(base, idx_expr) => {
                let base_val = self.eval_expr(base, Rc::clone(&env))?;
                let idx_val = self.eval_expr(idx_expr, env)?;
                match (base_val, idx_val) {
                    (Value::Vec(v), Value::Int(i)) => {
                        let idx = if i < 0 { v.len() as i64 + i } else { i } as usize;
                        v.get(idx).cloned()
                            .ok_or_else(|| err(format!("index {} out of bounds (len={})", i, v.len())))
                    }
                    (Value::HashMap(m), Value::Str(k)) => {
                        m.get(&k).cloned()
                            .ok_or_else(|| err(format!("key `{}` not found", k)))
                    }
                    (Value::Str(s), Value::Int(i)) => {
                        let idx = i as usize;
                        s.chars().nth(idx)
                            .map(|c| Value::Char(c))
                            .ok_or_else(|| err(format!("string index {} out of bounds", i)))
                    }
                    (Value::Tuple(v), Value::Int(i)) => {
                        let idx = if i < 0 { v.len() as i64 + i } else { i } as usize;
                        v.get(idx).cloned()
                            .ok_or_else(|| err(format!("tuple index {} out of bounds", i)))
                    }
                    (Value::Vec(v), Value::Range(lo, hi, inc)) => {
                        let lo = lo.max(0) as usize;
                        let hi = if hi == i64::MAX { v.len() }
                                 else if inc { (hi + 1).min(v.len() as i64) as usize }
                                 else { hi.min(v.len() as i64) as usize };
                        Ok(Value::Vec(v[lo.min(v.len())..hi].to_vec()))
                    }
                    (Value::Str(s), Value::Range(lo, hi, inc)) => {
                        let lo = lo.max(0) as usize;
                        let hi = if hi == i64::MAX { s.len() }
                                 else if inc { (hi + 1).min(s.len() as i64) as usize }
                                 else { hi.min(s.len() as i64) as usize };
                        Ok(Value::Str(s[lo.min(s.len())..hi].to_string()))
                    }
                    (b, i) => Err(err(format!("cannot index {} with {}", b.type_name(), i.type_name()))),
                }
            }

            Expr::If { cond, then_block, else_block } => {
                let cval = self.eval_expr(cond, Rc::clone(&env))?;
                let child = Rc::new(RefCell::new(Env::child(Rc::clone(&env))));
                if cval.is_truthy() {
                    self.eval_block(then_block, child)
                } else if let Some(else_expr) = else_block {
                    self.eval_expr(else_expr, env)
                } else {
                    Ok(Value::Unit)
                }
            }

            Expr::Block(block) => {
                let child = Rc::new(RefCell::new(Env::child(Rc::clone(&env))));
                self.eval_block(block, child)
            }

            Expr::Return(val_expr) => {
                let val = if let Some(e) = val_expr {
                    self.eval_expr(e, env)?
                } else {
                    Value::Unit
                };
                Err(Signal::Return(val))
            }

            Expr::Break(val_expr) => {
                let val = if let Some(e) = val_expr {
                    Some(self.eval_expr(e, env)?)
                } else {
                    None
                };
                Err(Signal::Break(val))
            }

            Expr::Continue => Err(Signal::Continue),

            Expr::Macro { name, args } => {
                self.eval_macro(name, args, env)
            }

            Expr::Match { scrutinee, arms } => {
                self.eval_match(scrutinee, arms, env)
            }

            Expr::Closure { params, body } => {
                use crate::ast::ClosureParam;
                let mut fn_params: Vec<Param> = Vec::new();
                let mut pre_stmts: Vec<Stmt> = Vec::new();
                for (i, cp) in params.iter().enumerate() {
                    match cp {
                        ClosureParam::Simple(name) => fn_params.push(Param {
                            name: name.clone(), ty: Ty::Unit, is_self: false, mutable: false,
                        }),
                        ClosureParam::Tuple(names) => {
                            let synth = format!("__p{}__", i);
                            fn_params.push(Param { name: synth.clone(), ty: Ty::Unit, is_self: false, mutable: false });
                            // Generate: let (a, b, ..) = __pN__;
                            for (j, n) in names.iter().enumerate() {
                                pre_stmts.push(Stmt::Let {
                                    name: n.clone(), mutable: true, ty: None,
                                    init: Some(Expr::Index(
                                        Box::new(Expr::Ident(synth.clone())),
                                        Box::new(Expr::Lit(Lit::Int(j as i64))),
                                    )),
                                });
                            }
                        }
                    }
                }
                let mut body_stmts = pre_stmts;
                body_stmts.push(Stmt::Expr(*body.clone()));
                let cfn = CrustFn {
                    params: fn_params,
                    ret_ty: None,
                    body: Block { stmts: body_stmts, tail: None },
                    captured: Some(Rc::clone(&env)),
                };
                Ok(Value::Fn(cfn))
            }

            Expr::StructLit { name, fields } => {
                let mut field_vals = HashMap::new();
                for (fname, fexpr) in fields {
                    let v = self.eval_expr(fexpr, Rc::clone(&env))?;
                    field_vals.insert(fname.clone(), v);
                }
                Ok(Value::Struct { type_name: name.clone(), fields: field_vals })
            }

            Expr::Array(elems) => {
                let vals: Vec<Value> = elems.iter()
                    .map(|e| self.eval_expr(e, Rc::clone(&env)))
                    .collect::<Result<_, _>>()?;
                Ok(Value::Vec(vals))
            }

            Expr::Tuple(elems) => {
                let vals: Vec<Value> = elems.iter()
                    .map(|e| self.eval_expr(e, Rc::clone(&env)))
                    .collect::<Result<_, _>>()?;
                Ok(Value::Tuple(vals))
            }

            Expr::Range { start, end, inclusive } => {
                let s = if let Some(e) = start { match self.eval_expr(e, Rc::clone(&env))? {
                    Value::Int(n) => n,
                    v => return Err(err(format!("range start must be integer, got {}", v.type_name()))),
                }} else { 0 };
                let e = if let Some(e) = end { match self.eval_expr(e, Rc::clone(&env))? {
                    Value::Int(n) => n,
                    v => return Err(err(format!("range end must be integer, got {}", v.type_name()))),
                }} else { i64::MAX };
                Ok(Value::Range(s, e, *inclusive))
            }

            Expr::Cast(inner, ty) => {
                let v = self.eval_expr(inner, env)?;
                let ty_name = match ty {
                    Ty::Named(s) => s.as_str(),
                    _ => "",
                };
                Ok(match (v, ty_name) {
                    (Value::Char(c), "i64"|"i32"|"u64"|"u32"|"u8"|"usize"|"isize") => Value::Int(c as i64),
                    (Value::Int(n), "char") => Value::Char(char::from_u32(n as u32).unwrap_or('\0')),
                    (Value::Int(n), "u8") => Value::Int(n & 0xFF),
                    (Value::Int(n), "f64"|"f32") => Value::Float(n as f64),
                    (Value::Float(f), "i64"|"i32"|"usize"|"isize") => Value::Int(f as i64),
                    (Value::Bool(b), "i64"|"i32") => Value::Int(b as i64),
                    (other, _) => other,
                })
            }

            Expr::Ref { expr, .. } => {
                // At Level 0, references are just the value itself (we clone everything)
                self.eval_expr(expr, env)
            }

            Expr::Deref(inner) => {
                let v = self.eval_expr(inner, Rc::clone(&env))?;
                if let Value::EntryRef { map_name, key } = v {
                    let map_val = lookup_entry_map(&map_name, &env)
                        .ok_or_else(|| err(format!("no map `{}`", map_name)))?;
                    if let Value::HashMap(m) = map_val {
                        return Ok(m.get(&key).cloned().unwrap_or(Value::Unit));
                    }
                    return Err(err(format!("`{}` is not a HashMap", map_name)));
                } else {
                    Ok(v)
                }
            }

            Expr::Try(inner) => {
                let v = self.eval_expr(inner, env)?;
                match v {
                    Value::Result_(Ok(inner)) => Ok(*inner),
                    Value::Result_(Err(e)) => Err(Signal::Return(Value::Result_(Err(e)))),
                    Value::Option_(Some(inner)) => Ok(*inner),
                    Value::Option_(None) => Err(Signal::Return(Value::Option_(None))),
                    other => Ok(other),
                }
            }
        }
    }

    // ── Macro evaluation ──────────────────────────────────────────────────────

    fn eval_macro(&mut self, name: &str, args: &[Expr], env: Rc<RefCell<Env>>) -> EvalResult {
        match name {
            "__for__" => self.eval_for_loop(args, env),

            "println" | "print" | "eprintln" | "eprint" => {
                let text = self.eval_format_macro(args, env)?;
                if name == "println" || name == "eprintln" {
                    let line = format!("{}", text);
                    println!("{}", line);
                    self.output.push(line);
                } else {
                    print!("{}", text);
                    self.output.push(text);
                }
                Ok(Value::Unit)
            }

            "format" => {
                let text = self.eval_format_macro(args, env)?;
                Ok(Value::Str(text))
            }

            "vec" => {
                let vals: Vec<Value> = args.iter()
                    .map(|a| self.eval_expr(a, Rc::clone(&env)))
                    .collect::<Result<_, _>>()?;
                Ok(Value::Vec(vals))
            }

            s if s.starts_with("__vec_repeat__") => {
                // vec![val; N] repeat: args[0] = value, args[1] = count
                if args.len() == 2 {
                    let val = self.eval_expr(&args[0], Rc::clone(&env))?;
                    let n = match self.eval_expr(&args[1], Rc::clone(&env))? {
                        Value::Int(n) => n as usize,
                        _ => 0,
                    };
                    Ok(Value::Vec(vec![val; n]))
                } else { Ok(Value::Vec(vec![])) }
            }

            "assert" => {
                if args.is_empty() { return Ok(Value::Unit); }
                let val = self.eval_expr(&args[0], Rc::clone(&env))?;
                if !val.is_truthy() {
                    let msg = if args.len() > 1 {
                        self.eval_format_macro(&args[1..], Rc::clone(&env))?
                    } else {
                        "assertion failed".to_string()
                    };
                    return Err(err(msg));
                }
                Ok(Value::Unit)
            }

            "assert_eq" => {
                if args.len() < 2 { return Ok(Value::Unit); }
                let a = self.eval_expr(&args[0], Rc::clone(&env))?;
                let b = self.eval_expr(&args[1], Rc::clone(&env))?;
                if !values_equal(&a, &b) {
                    return Err(err(format!("assertion failed: `(left == right)`\n  left: {}\n right: {}", a, b)));
                }
                Ok(Value::Unit)
            }

            "assert_ne" => {
                if args.len() < 2 { return Ok(Value::Unit); }
                let a = self.eval_expr(&args[0], Rc::clone(&env))?;
                let b = self.eval_expr(&args[1], Rc::clone(&env))?;
                if values_equal(&a, &b) {
                    return Err(err(format!("assertion failed: values are equal: {}", a)));
                }
                Ok(Value::Unit)
            }

            "panic" => {
                let msg = if args.is_empty() { "explicit panic".to_string() }
                          else { self.eval_format_macro(args, env)? };
                Err(err(msg))
            }

            "todo" => Err(err("not yet implemented")),
            "unimplemented" => Err(err("unimplemented")),
            "unreachable" => Err(err("entered unreachable code")),

            "dbg" => {
                let val = if args.is_empty() { Value::Unit }
                          else { self.eval_expr(&args[0], Rc::clone(&env))? };
                eprintln!("[dbg] {:?}", val.debug_repr());
                Ok(val)
            }

            "write" | "writeln" => {
                // ignore writer arg (args[0]), format the rest
                if args.len() < 2 { return Ok(Value::Result_(Ok(Box::new(Value::Unit)))); }
                let text = self.eval_format_macro(&args[1..], env)?;
                if name == "writeln" { println!("{}", text); } else { print!("{}", text); }
                Ok(Value::Result_(Ok(Box::new(Value::Unit))))
            }

            other => Err(err(format!("unknown macro: {}!", other))),
        }
    }

    fn eval_format_macro(&mut self, args: &[Expr], env: Rc<RefCell<Env>>) -> Result<String, Signal> {
        if args.is_empty() { return Ok(String::new()); }
        let fmt_val = self.eval_expr(&args[0], Rc::clone(&env))?;
        let fmt = match fmt_val {
            Value::Str(s) => s,
            other => return Ok(other.to_string()),
        };
        let rest: Vec<Value> = args[1..].iter()
            .map(|a| self.eval_expr(a, Rc::clone(&env)))
            .collect::<Result<_, _>>()?;
        crate::stdlib::format_string(&fmt, &rest).map_err(Signal::Err)
    }

    fn eval_for_loop(&mut self, args: &[Expr], env: Rc<RefCell<Env>>) -> EvalResult {
        // args: [pat_marker, iterable, body_block]
        if args.len() < 3 { return Ok(Value::Unit); }

        // Extract the pattern variable name from the marker
        let var_name = match &args[0] {
            Expr::Block(b) => {
                if let Some(Stmt::Expr(Expr::Ident(s))) = b.stmts.first() {
                    s.trim_start_matches("__pat__").to_string()
                } else { "_".to_string() }
            }
            _ => "_".to_string(),
        };

        let iterable = self.eval_expr(&args[1], Rc::clone(&env))?;
        let body = match &args[2] {
            Expr::Block(b) => b.clone(),
            _ => return Err(err("for loop body must be a block")),
        };

        let items: Vec<Value> = match iterable {
            Value::Vec(v) => v,
            Value::Range(start, end, inclusive) => {
                let end = if inclusive { end + 1 } else { end };
                (start..end).map(Value::Int).collect()
            }
            Value::Str(s) => s.chars().map(|c| Value::Str(c.to_string())).collect(),
            Value::HashMap(m) => {
                let mut pairs: Vec<_> = m.into_iter().collect();
                pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
                pairs.into_iter()
                    .map(|(k, v)| Value::Tuple(vec![Value::Str(k), v]))
                    .collect()
            }
            other => return Err(err(format!("cannot iterate over {}", other.type_name()))),
        };

        for item in items {
            let child = Rc::new(RefCell::new(Env::child(Rc::clone(&env))));
            // Bind the pattern variable
            bind_pattern_simple(&var_name, item, &mut child.borrow_mut());
            match self.eval_block(&body, Rc::clone(&child)) {
                Ok(_) => {}
                Err(Signal::Break(_)) => break,
                Err(Signal::Continue) => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(Value::Unit)
    }

    // ── Match evaluation ──────────────────────────────────────────────────────

    fn eval_match(&mut self, scrutinee_expr: &Expr, arms: &[MatchArm], env: Rc<RefCell<Env>>) -> EvalResult {
        let scrutinee = self.eval_expr(scrutinee_expr, Rc::clone(&env))?;

        // Special case: loop sentinel from parser
        if let [arm] = arms {
            if matches!(&arm.pat, Pat::Ident(s) if s == "__loop__") {
                return self.eval_loop_body(&arm.body, env);
            }
        }

        for arm in arms {
            let child = Rc::new(RefCell::new(Env::child(Rc::clone(&env))));
            if self.match_pat(&arm.pat, &scrutinee, &mut child.borrow_mut()) {
                if let Some(guard) = &arm.guard {
                    let gval = self.eval_expr(guard, Rc::clone(&child))?;
                    if !gval.is_truthy() { continue; }
                }
                return self.eval_expr(&arm.body, child);
            }
        }
        Ok(Value::Unit)
    }

    fn eval_loop_body(&mut self, body: &Expr, env: Rc<RefCell<Env>>) -> EvalResult {
        loop {
            let child = Rc::new(RefCell::new(Env::child(Rc::clone(&env))));
            match self.eval_expr(body, child) {
                Ok(_) => {}
                Err(Signal::Break(v)) => return Ok(v.unwrap_or(Value::Unit)),
                Err(Signal::Continue) => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn match_pat(&self, pat: &Pat, val: &Value, env: &mut Env) -> bool {
        match (pat, val) {
            (Pat::Wild, _) => true,
            (Pat::Ident(name), _) => {
                // Check if it's an enum variant name (None, Some, etc.) used as a pattern
                match (name.as_str(), val) {
                    ("None", Value::Option_(None)) => true,
                    ("None", _) => false, // None pattern only matches None
                    ("true", Value::Bool(true)) => true,
                    ("true", _) => false,
                    ("false", Value::Bool(false)) => true,
                    ("false", _) => false,
                    (n, _) if n.contains("::") => {
                        // Path pattern like MyEnum::Variant
                        match val {
                            Value::Enum { variant, .. } => n.ends_with(variant.as_str()),
                            Value::Struct { type_name, .. } => n == type_name || type_name.ends_with(&format!("::{}", n.rsplit("::").next().unwrap_or(n))),
                            _ => false,
                        }
                    }
                    // Uppercase single-word idents that look like enum variants — check against enums
                    (n, Value::Enum { variant, .. }) if n.chars().next().map_or(false, |c| c.is_uppercase()) => {
                        n == variant.as_str()
                    }
                    _ => {
                        // Binding pattern
                        env.define(name, val.clone());
                        true
                    }
                }
            }
            (Pat::Ref(inner), _) => self.match_pat(inner, val, env),
            (Pat::Lit(lit), _) => values_equal(&eval_lit(lit), val),
            (Pat::Tuple(pats), Value::Tuple(vals)) => {
                pats.len() == vals.len() &&
                    pats.iter().zip(vals.iter()).all(|(p, v)| self.match_pat(p, v, env))
            }
            (Pat::TupleStruct { name, fields }, val) => {
                match val {
                    Value::Enum { variant, inner, .. } => {
                        let matches_name = name == variant || name.ends_with(&format!("::{}", variant));
                        if !matches_name { return false; }
                        match (fields.as_slice(), inner) {
                            ([], None) => true,
                            ([single], Some(v)) => self.match_pat(single, v, env),
                            (multi, Some(v)) if multi.len() > 1 => {
                                if let Value::Tuple(vals) = v.as_ref() {
                                    multi.len() == vals.len() &&
                                        multi.iter().zip(vals.iter()).all(|(p, fv)| self.match_pat(p, fv, env))
                                } else { false }
                            }
                            _ => false,
                        }
                    }
                    Value::Option_(Some(v)) if name == "Some" => {
                        if fields.len() == 1 { self.match_pat(&fields[0], v, env) }
                        else { false }
                    }
                    Value::Option_(None) if name == "None" => true,
                    Value::Result_(Ok(v)) if name == "Ok" => {
                        if fields.len() == 1 { self.match_pat(&fields[0], v, env) }
                        else { false }
                    }
                    Value::Result_(Err(e)) if name == "Err" => {
                        if fields.len() == 1 { self.match_pat(&fields[0], e, env) }
                        else { false }
                    }
                    _ => false,
                }
            }
            (Pat::Struct { name, fields, .. }, Value::Struct { type_name, fields: fvals }) => {
                if name != type_name && !name.ends_with(type_name.as_str()) { return false; }
                for (fname, fpat) in fields {
                    if let Some(fval) = fvals.get(fname) {
                        if !self.match_pat(fpat, fval, env) { return false; }
                    } else { return false; }
                }
                true
            }
            (Pat::Or(pats), _) => pats.iter().any(|p| {
                let mut tmp = Env::child(Rc::new(RefCell::new(env.clone())));
                self.match_pat(p, val, &mut tmp)
            }),
            (Pat::Bind { name, pat }, _) => {
                if self.match_pat(pat, val, env) {
                    env.set(name, val.clone());
                    true
                } else {
                    false
                }
            }
            (Pat::Range(lo, hi, inc), Value::Int(n)) => {
                let lo_n = match eval_lit(lo) { Value::Int(n) => n, _ => return false };
                let hi_n = match eval_lit(hi) { Value::Int(n) => n, _ => return false };
                *n >= lo_n && if *inc { *n <= hi_n } else { *n < hi_n }
            }
            (Pat::Range(lo, hi, inc), Value::Char(c)) => {
                let lo_c = match eval_lit(lo) { Value::Char(c) => c, _ => return false };
                let hi_c = match eval_lit(hi) { Value::Char(c) => c, _ => return false };
                *c >= lo_c && if *inc { *c <= hi_c } else { *c < hi_c }
            }
            _ => false,
        }
    }

    // ── Assignment ────────────────────────────────────────────────────────────

    fn assign(&mut self, target: &Expr, val: Value, env: Rc<RefCell<Env>>) -> Result<(), Signal> {
        match target {
            Expr::Ident(name) => { env.borrow_mut().set(name, val); Ok(()) }
            Expr::Field(base, field) => {
                // Check if base is an EntryRef — field assignment writes through to the HashMap
                if let Expr::Ident(name) = base.as_ref() {
                    let base_raw = env.borrow().get(name);
                    if let Some(Value::EntryRef { map_name, key }) = base_raw {
                        let map_val = lookup_entry_map(&map_name, &env)
                            .ok_or_else(|| err(format!("no map `{}`", map_name)))?;
                        if let Value::HashMap(mut m) = map_val {
                            let entry_val = m.get(&key).cloned().unwrap_or(Value::Unit);
                            let updated = match entry_val {
                                Value::Tuple(mut t) => {
                                    let idx: usize = field.parse().unwrap_or(0);
                                    if idx < t.len() { t[idx] = val; }
                                    Value::Tuple(t)
                                }
                                Value::Struct { type_name, mut fields } => {
                                    fields.insert(field.clone(), val);
                                    Value::Struct { type_name, fields }
                                }
                                other => other,
                            };
                            m.insert(key, updated);
                            write_back_entry_map(&map_name, Value::HashMap(m), &env);
                            return Ok(());
                        }
                    }
                }
                let mut struct_val = self.eval_expr(base, Rc::clone(&env))?;
                match &mut struct_val {
                    Value::Struct { ref mut fields, .. } => {
                        fields.insert(field.clone(), val);
                        self.assign(base, struct_val, env)
                    }
                    Value::Tuple(ref mut t) => {
                        let idx: usize = field.parse().unwrap_or(0);
                        if idx < t.len() { t[idx] = val; }
                        self.assign(base, struct_val, env)
                    }
                    _ => Err(err(format!("cannot assign field `{}` on non-struct", field))),
                }
            }
            Expr::Index(base, idx_expr) => {
                let mut vec_val = self.eval_expr(base, Rc::clone(&env))?;
                let idx = self.eval_expr(idx_expr, Rc::clone(&env))?;
                match (&mut vec_val, idx) {
                    (Value::Vec(ref mut v), Value::Int(i)) => {
                        let idx = if i < 0 { v.len() as i64 + i } else { i } as usize;
                        if idx < v.len() { v[idx] = val; }
                        self.assign(base, vec_val, env)
                    }
                    _ => Err(err("invalid index assignment")),
                }
            }
            Expr::Deref(inner) => {
                // If deref target is an EntryRef, write through to the map
                if let Expr::Ident(name) = inner.as_ref() {
                    let current = env.borrow().get(name);
                    if let Some(Value::EntryRef { map_name, key }) = current {
                        let mut map_val = lookup_entry_map(&map_name, &env)
                            .ok_or_else(|| err(format!("no map `{}`", map_name)))?;
                        if let Value::HashMap(ref mut m) = map_val {
                            m.insert(key, val);
                        }
                        write_back_entry_map(&map_name, map_val, &env);
                        return Ok(());
                    }
                } else {
                    // e.g. *counts.entry(k).or_insert(0) += 1
                    let inner_val = self.eval_expr(inner, Rc::clone(&env))?;
                    if let Value::EntryRef { map_name, key } = inner_val {
                        let mut map_val = lookup_entry_map(&map_name, &env)
                            .ok_or_else(|| err(format!("no map `{}`", map_name)))?;
                        if let Value::HashMap(ref mut m) = map_val {
                            m.insert(key, val);
                        }
                        write_back_entry_map(&map_name, map_val, &env);
                        return Ok(());
                    }
                }
                self.assign(inner, val, env)
            }
            Expr::Ref { expr, .. } => self.assign(expr, val, env),
            _ => Err(err("invalid assignment target")),
        }
    }

    // ── Method / static call dispatch ─────────────────────────────────────────

    pub fn call_method_or_static(&mut self, type_name: &str, method: &str, self_val: Option<Value>, args: Vec<Value>, env: Rc<RefCell<Env>>) -> EvalResult {
        // 1. User-defined impl methods — try exact type, then enum prefix (AppError::InvalidInput → AppError)
        let user_method = self.impls.get(type_name)
            .and_then(|methods| methods.iter().find(|m| m.name == method))
            .cloned()
            .or_else(|| {
                if let Some(prefix) = type_name.rfind("::").map(|i| &type_name[..i]) {
                    self.impls.get(prefix)
                        .and_then(|methods| methods.iter().find(|m| m.name == method))
                        .cloned()
                } else { None }
            });

        if let Some(fdef) = user_method {
            let cfn = CrustFn { params: fdef.params, ret_ty: fdef.ret_ty, body: fdef.body, captured: None };
            return self.call_crust_fn(&cfn, args, self_val);
        }

        // 2. Built-in methods (instance or static) from stdlib
        if let Some(r) = crate::stdlib::call_method(type_name, method, self_val.clone(), args.clone(), self) {
            return r;
        }

        // 3. Built-in free functions registered as "Type::method" (e.g. Vec::new, HashMap::new)
        let qualified = format!("{}::{}", type_name, method);
        if let Some(r) = crate::stdlib::call_builtin(&qualified, args.clone(), self) {
            return r;
        }

        // 4. Enum variant construction: Shape::Circle(x) or Shape::Unit
        if method.chars().next().map_or(false, |c| c.is_uppercase()) {
            let inner = match args.len() {
                0 => None,
                1 => Some(Box::new(args.into_iter().next().unwrap())),
                _ => Some(Box::new(Value::Tuple(args))),
            };
            return Ok(Value::Enum { type_name: type_name.to_string(), variant: method.to_string(), inner });
        }

        Err(err(format!("no method `{}` on type `{}`", method, type_name)))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn apply_mut_writeback(arg_expr: Option<&Expr>, new_val: Value, env: &Rc<RefCell<Env>>) {
    match arg_expr {
        Some(Expr::Ident(name)) => { env.borrow_mut().set(name, new_val); }
        Some(Expr::Ref { expr, .. }) => {
            if let Expr::Ident(name) = expr.as_ref() {
                env.borrow_mut().set(name, new_val);
            }
        }
        _ => {}
    }
}

pub fn compare_values(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y))     => x.partial_cmp(y),
        (Float(x), Float(y)) => x.partial_cmp(y),
        (Int(x), Float(y))   => (*x as f64).partial_cmp(y),
        (Float(x), Int(y))   => x.partial_cmp(&(*y as f64)),
        (Str(x), Str(y))     => x.partial_cmp(y),
        (Bool(x), Bool(y))   => x.partial_cmp(y),
        (Char(x), Char(y))   => x.partial_cmp(y),
        (Tuple(a), Tuple(b)) => {
            for (x, y) in a.iter().zip(b.iter()) {
                match compare_values(x, y) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            a.len().partial_cmp(&b.len())
        }
        _                    => None,
    }
}

pub fn eval_lit(lit: &Lit) -> Value {
    match lit {
        Lit::Int(n)   => Value::Int(*n),
        Lit::Float(f) => Value::Float(*f),
        Lit::Bool(b)  => Value::Bool(*b),
        Lit::Str(s)   => Value::Str(s.clone()),
        Lit::Char(c)  => Value::Char(*c),
    }
}

fn eval_unary(op: &UnOp, val: Value) -> EvalResult {
    match (op, val) {
        (UnOp::Neg, Value::Int(n))   => Ok(Value::Int(-n)),
        (UnOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
        (UnOp::Not, Value::Bool(b))  => Ok(Value::Bool(!b)),
        (UnOp::Not, Value::Int(n))   => Ok(Value::Int(!n)),
        (op, v) => Err(err(format!("cannot apply {:?} to {}", op, v.type_name()))),
    }
}

pub fn eval_binary(op: &BinOp, l: Value, r: Value) -> EvalResult {
    use Value::*;
    match (op, &l, &r) {
        // Integer arithmetic
        (BinOp::Add, Int(a), Int(b)) => Ok(Int(a + b)),
        (BinOp::Sub, Int(a), Int(b)) => Ok(Int(a - b)),
        (BinOp::Mul, Int(a), Int(b)) => Ok(Int(a * b)),
        (BinOp::Div, Int(a), Int(b)) => {
            if *b == 0 { return Err(err("division by zero")); }
            Ok(Int(a / b))
        }
        (BinOp::Rem, Int(a), Int(b)) => {
            if *b == 0 { return Err(err("remainder by zero")); }
            Ok(Int(a % b))
        }
        (BinOp::BitAnd, Int(a), Int(b)) => Ok(Int(a & b)),
        (BinOp::BitOr,  Int(a), Int(b)) => Ok(Int(a | b)),
        (BinOp::BitXor, Int(a), Int(b)) => Ok(Int(a ^ b)),
        (BinOp::Shl,    Int(a), Int(b)) => Ok(Int(a << b)),
        (BinOp::Shr,    Int(a), Int(b)) => Ok(Int(a >> b)),

        // Float arithmetic
        (BinOp::Add, Float(a), Float(b)) => Ok(Float(a + b)),
        (BinOp::Sub, Float(a), Float(b)) => Ok(Float(a - b)),
        (BinOp::Mul, Float(a), Float(b)) => Ok(Float(a * b)),
        (BinOp::Div, Float(a), Float(b)) => Ok(Float(a / b)),
        (BinOp::Rem, Float(a), Float(b)) => Ok(Float(a % b)),

        // Int+Float mixed
        (BinOp::Add, Int(a), Float(b))   => Ok(Float(*a as f64 + b)),
        (BinOp::Add, Float(a), Int(b))   => Ok(Float(a + *b as f64)),
        (BinOp::Sub, Int(a), Float(b))   => Ok(Float(*a as f64 - b)),
        (BinOp::Sub, Float(a), Int(b))   => Ok(Float(a - *b as f64)),
        (BinOp::Mul, Int(a), Float(b))   => Ok(Float(*a as f64 * b)),
        (BinOp::Mul, Float(a), Int(b))   => Ok(Float(a * *b as f64)),
        (BinOp::Div, Int(a), Float(b))   => Ok(Float(*a as f64 / b)),
        (BinOp::Div, Float(a), Int(b))   => Ok(Float(a / *b as f64)),

        // String concatenation
        (BinOp::Add, Str(a), Str(b))   => Ok(Str(format!("{}{}", a, b))),
        (BinOp::Add, Str(a), Int(b))   => Ok(Str(format!("{}{}", a, b))),
        (BinOp::Add, Str(a), Float(b)) => Ok(Str(format!("{}{}", a, b))),

        // Comparisons — numeric
        (BinOp::Eq, Int(a),   Int(b))   => Ok(Bool(a == b)),
        (BinOp::Ne, Int(a),   Int(b))   => Ok(Bool(a != b)),
        (BinOp::Lt, Int(a),   Int(b))   => Ok(Bool(a < b)),
        (BinOp::Le, Int(a),   Int(b))   => Ok(Bool(a <= b)),
        (BinOp::Gt, Int(a),   Int(b))   => Ok(Bool(a > b)),
        (BinOp::Ge, Int(a),   Int(b))   => Ok(Bool(a >= b)),
        (BinOp::Eq, Float(a), Float(b)) => Ok(Bool(a == b)),
        (BinOp::Ne, Float(a), Float(b)) => Ok(Bool(a != b)),
        (BinOp::Lt, Float(a), Float(b)) => Ok(Bool(a < b)),
        (BinOp::Le, Float(a), Float(b)) => Ok(Bool(a <= b)),
        (BinOp::Gt, Float(a), Float(b)) => Ok(Bool(a > b)),
        (BinOp::Ge, Float(a), Float(b)) => Ok(Bool(a >= b)),
        (BinOp::Eq, Int(a),   Float(b)) => Ok(Bool((*a as f64) == *b)),
        (BinOp::Ne, Int(a),   Float(b)) => Ok(Bool((*a as f64) != *b)),
        (BinOp::Eq, Float(a), Int(b))   => Ok(Bool(*a == (*b as f64))),
        (BinOp::Ne, Float(a), Int(b))   => Ok(Bool(*a != (*b as f64))),
        (BinOp::Lt, Int(a),   Float(b)) => Ok(Bool((*a as f64) < *b)),
        (BinOp::Le, Int(a),   Float(b)) => Ok(Bool((*a as f64) <= *b)),
        (BinOp::Gt, Int(a),   Float(b)) => Ok(Bool((*a as f64) > *b)),
        (BinOp::Ge, Int(a),   Float(b)) => Ok(Bool((*a as f64) >= *b)),
        (BinOp::Lt, Float(a), Int(b))   => Ok(Bool(*a < (*b as f64))),
        (BinOp::Le, Float(a), Int(b))   => Ok(Bool(*a <= (*b as f64))),
        (BinOp::Gt, Float(a), Int(b))   => Ok(Bool(*a > (*b as f64))),
        (BinOp::Ge, Float(a), Int(b))   => Ok(Bool(*a >= (*b as f64))),

        // Comparisons — strings, bools, chars
        (BinOp::Eq, Str(a),  Str(b))  => Ok(Bool(a == b)),
        (BinOp::Ne, Str(a),  Str(b))  => Ok(Bool(a != b)),
        (BinOp::Lt, Str(a),  Str(b))  => Ok(Bool(a < b)),
        (BinOp::Le, Str(a),  Str(b))  => Ok(Bool(a <= b)),
        (BinOp::Gt, Str(a),  Str(b))  => Ok(Bool(a > b)),
        (BinOp::Ge, Str(a),  Str(b))  => Ok(Bool(a >= b)),
        (BinOp::Eq, Bool(a), Bool(b)) => Ok(Bool(a == b)),
        (BinOp::Ne, Bool(a), Bool(b)) => Ok(Bool(a != b)),
        (BinOp::Eq, Char(a), Char(b)) => Ok(Bool(a == b)),
        (BinOp::Ne, Char(a), Char(b)) => Ok(Bool(a != b)),
        (BinOp::Lt, Char(a), Char(b)) => Ok(Bool(a < b)),
        (BinOp::Le, Char(a), Char(b)) => Ok(Bool(a <= b)),
        (BinOp::Gt, Char(a), Char(b)) => Ok(Bool(a > b)),
        (BinOp::Ge, Char(a), Char(b)) => Ok(Bool(a >= b)),

        // General equality fallback
        (BinOp::Eq, a, b) => Ok(Bool(values_equal(a, b))),
        (BinOp::Ne, a, b) => Ok(Bool(!values_equal(a, b))),

        (op, l, r) => Err(err(format!("cannot apply {:?} to {} and {}", op, l.type_name(), r.type_name()))),
    }
}

pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x),   Value::Int(y))   => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Int(x),   Value::Float(y)) => (*x as f64) == *y,
        (Value::Float(x), Value::Int(y))   => *x == (*y as f64),
        (Value::Bool(x),  Value::Bool(y))  => x == y,
        (Value::Str(x),   Value::Str(y))   => x == y,
        (Value::Char(x),  Value::Char(y))  => x == y,
        (Value::Unit,     Value::Unit)     => true,
        (Value::Option_(None), Value::Option_(None)) => true,
        (Value::Option_(Some(a)), Value::Option_(Some(b))) => values_equal(a, b),
        (Value::Enum { variant: va, inner: None, .. }, Value::Enum { variant: vb, inner: None, .. }) => va == vb,
        (Value::Enum { variant: va, inner: Some(ia), .. }, Value::Enum { variant: vb, inner: Some(ib), .. }) => {
            va == vb && values_equal(ia, ib)
        }
        (Value::Tuple(a), Value::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_equal(x, y))
        }
        (Value::Vec(a), Value::Vec(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_equal(x, y))
        }
        _ => false,
    }
}

fn bind_pattern_simple(pat: &str, val: Value, env: &mut Env) {
    if pat.starts_with('(') {
        // tuple pattern: (a,b,c)
        let inner = &pat[1..pat.len()-1];
        let names: Vec<&str> = inner.split(',').collect();
        if let Value::Tuple(vals) = val {
            for (name, v) in names.iter().zip(vals.into_iter()) {
                env.define(name.trim(), v);
            }
        }
    } else if pat != "_" {
        env.define(pat, val);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn run(src: &str) -> Result<Vec<String>, CrustError> {
        let tokens = Lexer::new(src).tokenize()?;
        let prog = Parser::new(tokens).parse_program()?;
        let mut interp = Interpreter::new();
        interp.run(prog)?;
        Ok(interp.output)
    }

    #[test]
    fn eval_fib() {
        let src = "fn fib(n: u64) -> u64 {
            if n <= 1 { return n; }
            fib(n - 1) + fib(n - 2)
        }
        fn main() {
            let result = fib(10);
            println!(\"{}\", result);
        }";
        let out = run(src).unwrap();
        assert_eq!(out, vec!["55"]);
    }

    #[test]
    fn eval_for_loop() {
        let src = "fn main() {
            let v = vec![1, 2, 3];
            for x in v {
                println!(\"{}\", x);
            }
        }";
        let out = run(src).unwrap();
        assert_eq!(out, vec!["1", "2", "3"]);
    }

    #[test]
    fn eval_struct() {
        let src = "
        struct Point { x: f64, y: f64 }
        fn main() {
            let p = Point { x: 3.0, y: 4.0 };
            println!(\"{}\", p.x);
        }";
        let out = run(src).unwrap();
        assert_eq!(out, vec!["3"]);
    }
}
