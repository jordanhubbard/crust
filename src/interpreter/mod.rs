pub mod env;
pub mod error;
pub mod value;

use env::Environment;
use error::CrustError;
use value::Value;

use std::cell::RefCell;
use std::rc::Rc;

use syn::{self, BinOp, Expr, Item, Lit, Pat, Stmt, UnOp};

/// The Crust tree-walking interpreter.
/// Walks a `syn` AST and evaluates it directly — no intermediate representation.
pub struct Interpreter {
    /// Global environment (functions, top-level bindings)
    pub global_env: Rc<RefCell<Environment>>,
    /// Pedantic level: 0=hack, 1=type-strict, 2=ownership, 3=full Rust
    pub pedantic: u8,
}

impl Interpreter {
    pub fn new(pedantic: u8) -> Self {
        let global_env = Rc::new(RefCell::new(Environment::new()));
        crate::stdlib::register_builtins(&global_env);
        Self { global_env, pedantic }
    }

    /// Parse and run a complete Rust source file.
    pub fn run(&mut self, source: &str, filename: &str) -> Result<(), CrustError> {
        let file: syn::File = syn::parse_str(source)
            .map_err(|e| CrustError::Parse(format!("{}: {}", filename, e)))?;

        // First pass: register all function definitions
        for item in &file.items {
            self.register_item(item, &self.global_env.clone())?;
        }

        // Second pass: call main()
        let main_fn = self.global_env.borrow().get("main")
            .ok_or_else(|| CrustError::Runtime("no `main` function found".into()))?;

        if let Value::Function { params, body, closure_env, .. } = main_fn {
            if !params.is_empty() {
                return Err(CrustError::Runtime("main() must take no arguments".into()));
            }
            let fn_env = Rc::new(RefCell::new(Environment::with_parent(closure_env)));
            self.exec_block(&body, &fn_env)?;
            Ok(())
        } else {
            Err(CrustError::Runtime("`main` is not a function".into()))
        }
    }

    /// Evaluate a REPL input — tries as item, statement, or expression.
    pub fn eval_repl(&mut self, input: &str) -> Result<Value, CrustError> {
        // Try as a full item (fn, struct, etc.)
        if let Ok(item) = syn::parse_str::<syn::Item>(input) {
            self.register_item(&item, &self.global_env.clone())?;
            return Ok(Value::Unit);
        }

        // Try as a statement
        if let Ok(file) = syn::parse_str::<syn::File>(&format!("fn __repl__() {{ {} }}", input)) {
            if let Some(Item::Fn(f)) = file.items.first() {
                let env = self.global_env.clone();
                for stmt in &f.block.stmts {
                    let val = self.exec_stmt(stmt, &env)?;
                    // Return the last expression value
                    if matches!(stmt, Stmt::Expr(_, None)) {
                        return Ok(val);
                    }
                }
                return Ok(Value::Unit);
            }
        }

        // Try as an expression
        if let Ok(expr) = syn::parse_str::<syn::Expr>(input) {
            return self.eval_expr(&expr, &self.global_env.clone());
        }

        Err(CrustError::Parse(format!("could not parse: {}", input)))
    }

    /// Register a top-level item (fn, struct, etc.) in the given environment.
    fn register_item(&self, item: &Item, env: &Rc<RefCell<Environment>>) -> Result<(), CrustError> {
        match item {
            Item::Fn(f) => {
                let name = f.sig.ident.to_string();
                let params: Vec<(String, String)> = f.sig.inputs.iter().map(|arg| {
                    match arg {
                        syn::FnArg::Typed(pat_type) => {
                            let param_name = pat_to_string(&pat_type.pat);
                            let param_type = type_to_string(&pat_type.ty);
                            (param_name, param_type)
                        }
                        syn::FnArg::Receiver(_) => ("self".into(), "Self".into()),
                    }
                }).collect();

                let return_type = match &f.sig.output {
                    syn::ReturnType::Default => None,
                    syn::ReturnType::Type(_, ty) => Some(type_to_string(ty)),
                };

                let body: Vec<Stmt> = f.block.stmts.clone();

                env.borrow_mut().set(name, Value::Function {
                    params,
                    body,
                    closure_env: env.clone(),
                    return_type,
                });
                Ok(())
            }
            Item::Struct(s) => {
                let name = s.ident.to_string();
                let fields: Vec<(String, String)> = match &s.fields {
                    syn::Fields::Named(named) => {
                        named.named.iter().map(|f| {
                            let fname = f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();
                            let ftype = type_to_string(&f.ty);
                            (fname, ftype)
                        }).collect()
                    }
                    _ => vec![],
                };
                env.borrow_mut().set(name, Value::StructDef { fields });
                Ok(())
            }
            _ => Ok(()), // Skip other items for now
        }
    }

    /// Execute a block of statements, returning the value of the last expression.
    pub fn exec_block(&self, stmts: &[Stmt], env: &Rc<RefCell<Environment>>) -> Result<Value, CrustError> {
        let mut last = Value::Unit;
        for stmt in stmts {
            last = self.exec_stmt(stmt, env)?;
        }
        Ok(last)
    }

    /// Execute a single statement.
    fn exec_stmt(&self, stmt: &Stmt, env: &Rc<RefCell<Environment>>) -> Result<Value, CrustError> {
        match stmt {
            Stmt::Local(local) => {
                let val = if let Some(init) = &local.init {
                    self.eval_expr(&init.expr, env)?
                } else {
                    Value::Unit
                };
                self.bind_pattern(&local.pat, val, env)?;
                Ok(Value::Unit)
            }
            Stmt::Expr(expr, _semi) => {
                self.eval_expr(expr, env)
            }
            Stmt::Item(item) => {
                self.register_item(item, env)?;
                Ok(Value::Unit)
            }
            _ => Ok(Value::Unit),
        }
    }

    /// Bind a pattern to a value (let destructuring).
    fn bind_pattern(&self, pat: &Pat, val: Value, env: &Rc<RefCell<Environment>>) -> Result<(), CrustError> {
        match pat {
            Pat::Ident(ident) => {
                env.borrow_mut().set(ident.ident.to_string(), val);
                Ok(())
            }
            Pat::Tuple(tuple) => {
                if let Value::Tuple(vals) = val {
                    for (p, v) in tuple.elems.iter().zip(vals.into_iter()) {
                        self.bind_pattern(p, v, env)?;
                    }
                    Ok(())
                } else {
                    Err(CrustError::Runtime("cannot destructure non-tuple".into()))
                }
            }
            Pat::Type(pat_type) => {
                self.bind_pattern(&pat_type.pat, val, env)
            }
            Pat::Wild(_) => Ok(()), // _ pattern, discard
            _ => {
                env.borrow_mut().set(format!("{}", quote::quote!(#pat)), val);
                Ok(())
            }
        }
    }

    /// Evaluate an expression and return its value.
    pub fn eval_expr(&self, expr: &Expr, env: &Rc<RefCell<Environment>>) -> Result<Value, CrustError> {
        match expr {
            // Literals: 42, 3.14, true, "hello"
            Expr::Lit(lit) => self.eval_lit(&lit.lit),

            // Variable reference
            Expr::Path(path) => {
                let name = path_to_string(&path.path);
                // Check for boolean literals
                match name.as_str() {
                    "true" => return Ok(Value::Bool(true)),
                    "false" => return Ok(Value::Bool(false)),
                    "None" => return Ok(Value::Option(None)),
                    _ => {}
                }
                env.borrow().get(&name)
                    .ok_or_else(|| CrustError::Runtime(format!("undefined variable: `{}`", name)))
            }

            // Binary operations: a + b, a == b, etc.
            Expr::Binary(bin) => {
                let left = self.eval_expr(&bin.left, env)?;
                // Short-circuit for && and ||
                match bin.op {
                    BinOp::And(_) => {
                        if !left.as_bool()? {
                            return Ok(Value::Bool(false));
                        }
                        let right = self.eval_expr(&bin.right, env)?;
                        return Ok(Value::Bool(right.as_bool()?));
                    }
                    BinOp::Or(_) => {
                        if left.as_bool()? {
                            return Ok(Value::Bool(true));
                        }
                        let right = self.eval_expr(&bin.right, env)?;
                        return Ok(Value::Bool(right.as_bool()?));
                    }
                    _ => {}
                }
                let right = self.eval_expr(&bin.right, env)?;
                self.eval_binop(&bin.op, left, right)
            }

            // Unary: -x, !x, *x
            Expr::Unary(un) => {
                match un.op {
                    UnOp::Deref(_) => self.eval_expr(&un.expr, env), // hack mode: deref is passthrough
                    _ => {
                        let val = self.eval_expr(&un.expr, env)?;
                        match un.op {
                            UnOp::Neg(_) => val.negate(),
                            UnOp::Not(_) => val.not(),
                            _ => Err(CrustError::Runtime("unsupported unary operator".into())),
                        }
                    }
                }
            }

            // Block: { ... }
            Expr::Block(block) => {
                let block_env = Rc::new(RefCell::new(Environment::with_parent(env.clone())));
                self.exec_block(&block.block.stmts, &block_env)
            }

            // If/else
            Expr::If(if_expr) => {
                let cond = self.eval_expr(&if_expr.cond, env)?;
                if cond.as_bool()? {
                    let block_env = Rc::new(RefCell::new(Environment::with_parent(env.clone())));
                    self.exec_block(&if_expr.then_branch.stmts, &block_env)
                } else if let Some((_, else_expr)) = &if_expr.else_branch {
                    self.eval_expr(else_expr, env)
                } else {
                    Ok(Value::Unit)
                }
            }

            // While loop
            Expr::While(while_expr) => {
                loop {
                    let cond = self.eval_expr(&while_expr.cond, env)?;
                    if !cond.as_bool()? {
                        break;
                    }
                    let loop_env = Rc::new(RefCell::new(Environment::with_parent(env.clone())));
                    match self.exec_block(&while_expr.body.stmts, &loop_env) {
                        Ok(_) => {}
                        Err(CrustError::Break(v)) => return Ok(v),
                        Err(CrustError::Continue) => continue,
                        Err(e) => return Err(e),
                    }
                }
                Ok(Value::Unit)
            }

            // Loop
            Expr::Loop(loop_expr) => {
                loop {
                    let loop_env = Rc::new(RefCell::new(Environment::with_parent(env.clone())));
                    match self.exec_block(&loop_expr.body.stmts, &loop_env) {
                        Ok(_) => {}
                        Err(CrustError::Break(v)) => return Ok(v),
                        Err(CrustError::Continue) => continue,
                        Err(e) => return Err(e),
                    }
                }
            }

            // For loop: for x in iter { ... }
            Expr::ForLoop(for_loop) => {
                let iter_val = self.eval_expr(&for_loop.expr, env)?;
                let items = iter_val.into_iter()?;
                for item in items {
                    let loop_env = Rc::new(RefCell::new(Environment::with_parent(env.clone())));
                    self.bind_pattern(&for_loop.pat, item, &loop_env)?;
                    match self.exec_block(&for_loop.body.stmts, &loop_env) {
                        Ok(_) => {}
                        Err(CrustError::Break(v)) => return Ok(v),
                        Err(CrustError::Continue) => continue,
                        Err(e) => return Err(e),
                    }
                }
                Ok(Value::Unit)
            }

            // Range: 0..n, 0..=n
            Expr::Range(range) => {
                let start = if let Some(from) = &range.start {
                    self.eval_expr(from, env)?.as_i64()?
                } else {
                    0
                };
                let end = if let Some(to) = &range.end {
                    self.eval_expr(to, env)?.as_i64()?
                } else {
                    return Err(CrustError::Runtime("range must have an end".into()));
                };

                let items: Vec<Value> = match range.limits {
                    syn::RangeLimits::HalfOpen(_) => (start..end).map(Value::Int).collect(),
                    syn::RangeLimits::Closed(_) => (start..=end).map(Value::Int).collect(),
                };
                Ok(Value::Vec(Rc::new(RefCell::new(items))))
            }

            // Function/method call: foo(args), x.method(args)
            Expr::Call(call) => {
                self.eval_call(call, env)
            }

            // Method call: x.push(val), x.len()
            Expr::MethodCall(method) => {
                self.eval_method_call(method, env)
            }

            // Field access: x.field
            Expr::Field(field) => {
                let obj = self.eval_expr(&field.base, env)?;
                let field_name = match &field.member {
                    syn::Member::Named(ident) => ident.to_string(),
                    syn::Member::Unnamed(idx) => idx.index.to_string(),
                };
                obj.get_field(&field_name)
            }

            // Index: x[i]
            Expr::Index(idx) => {
                let obj = self.eval_expr(&idx.expr, env)?;
                let index = self.eval_expr(&idx.index, env)?;
                obj.index(&index)
            }

            // Assignment: x = val
            Expr::Assign(assign) => {
                let val = self.eval_expr(&assign.right, env)?;
                self.assign_expr(&assign.left, val, env)?;
                Ok(Value::Unit)
            }

            // Match expression
            Expr::Match(match_expr) => {
                let scrutinee = self.eval_expr(&match_expr.expr, env)?;
                for arm in &match_expr.arms {
                    if let Some(val) = self.try_match_arm(&scrutinee, arm, env)? {
                        return Ok(val);
                    }
                }
                Err(CrustError::Runtime("non-exhaustive match".into()))
            }

            // Return
            Expr::Return(ret) => {
                let val = if let Some(expr) = &ret.expr {
                    self.eval_expr(expr, env)?
                } else {
                    Value::Unit
                };
                Err(CrustError::Return(val))
            }

            // Break
            Expr::Break(brk) => {
                let val = if let Some(expr) = &brk.expr {
                    self.eval_expr(expr, env)?
                } else {
                    Value::Unit
                };
                Err(CrustError::Break(val))
            }

            // Continue
            Expr::Continue(_) => {
                Err(CrustError::Continue)
            }

            // Macro invocations: println!(...), vec![...]
            Expr::Macro(mac) => {
                self.eval_macro(mac, env)
            }

            // Tuple: (a, b, c)
            Expr::Tuple(tuple) => {
                let vals: Result<Vec<Value>, _> = tuple.elems.iter()
                    .map(|e| self.eval_expr(e, env))
                    .collect();
                Ok(Value::Tuple(vals?))
            }

            // Array: [1, 2, 3]
            Expr::Array(arr) => {
                let vals: Result<Vec<Value>, _> = arr.elems.iter()
                    .map(|e| self.eval_expr(e, env))
                    .collect();
                Ok(Value::Vec(Rc::new(RefCell::new(vals?))))
            }

            // Parenthesized: (expr)
            Expr::Paren(paren) => self.eval_expr(&paren.expr, env),

            // Struct literal: Foo { x: 1, y: 2 }
            Expr::Struct(s) => {
                let mut fields = std::collections::HashMap::new();
                let name = path_to_string(&s.path);
                for field in &s.fields {
                    let fname = match &field.member {
                        syn::Member::Named(ident) => ident.to_string(),
                        syn::Member::Unnamed(idx) => idx.index.to_string(),
                    };
                    let fval = self.eval_expr(&field.expr, env)?;
                    fields.insert(fname, fval);
                }
                Ok(Value::Struct { name, fields })
            }

            // Closure: |x| x + 1
            Expr::Closure(closure) => {
                let params: Vec<(String, String)> = closure.inputs.iter().map(|p| {
                    (pat_to_string(p), String::new())
                }).collect();
                // Wrap the body expression as a return statement
                let body_expr = &closure.body;
                let body_stmt = syn::parse_quote! { return #body_expr; };
                Ok(Value::Function {
                    params,
                    body: vec![body_stmt],
                    closure_env: env.clone(),
                    return_type: None,
                })
            }

            // Reference: &x, &mut x — in hack mode, just pass through
            Expr::Reference(r) => self.eval_expr(&r.expr, env),

            // Cast: x as i32
            Expr::Cast(cast) => {
                let val = self.eval_expr(&cast.expr, env)?;
                let target = type_to_string(&cast.ty);
                val.cast_to(&target)
            }

            // Repeat array: [0; 10]
            Expr::Repeat(rep) => {
                let val = self.eval_expr(&rep.expr, env)?;
                let len = self.eval_expr(&rep.len, env)?.as_i64()? as usize;
                let items: Vec<Value> = (0..len).map(|_| val.clone()).collect();
                Ok(Value::Vec(Rc::new(RefCell::new(items))))
            }

            // Let expression (if let): handled as special case
            Expr::Let(_) => {
                // Simplified — should be handled as part of if-let
                Ok(Value::Bool(false))
            }

            _ => Err(CrustError::Runtime(format!("unsupported expression: {}", quote::quote!(#expr)))),
        }
    }

    /// Evaluate a literal.
    fn eval_lit(&self, lit: &Lit) -> Result<Value, CrustError> {
        match lit {
            Lit::Int(i) => {
                let n: i64 = i.base10_parse()
                    .map_err(|e| CrustError::Runtime(format!("invalid integer: {}", e)))?;
                Ok(Value::Int(n))
            }
            Lit::Float(f) => {
                let n: f64 = f.base10_parse()
                    .map_err(|e| CrustError::Runtime(format!("invalid float: {}", e)))?;
                Ok(Value::Float(n))
            }
            Lit::Str(s) => Ok(Value::Str(s.value())),
            Lit::Bool(b) => Ok(Value::Bool(b.value)),
            Lit::Char(c) => Ok(Value::Char(c.value())),
            _ => Err(CrustError::Runtime("unsupported literal type".into())),
        }
    }

    /// Evaluate a binary operation.
    fn eval_binop(&self, op: &BinOp, left: Value, right: Value) -> Result<Value, CrustError> {
        match op {
            BinOp::Add(_) => left.add(&right),
            BinOp::Sub(_) => left.sub(&right),
            BinOp::Mul(_) => left.mul(&right),
            BinOp::Div(_) => left.div(&right),
            BinOp::Rem(_) => left.rem(&right),
            BinOp::Eq(_) => Ok(Value::Bool(left == right)),
            BinOp::Ne(_) => Ok(Value::Bool(left != right)),
            BinOp::Lt(_) => left.lt_val(&right),
            BinOp::Le(_) => left.le_val(&right),
            BinOp::Gt(_) => left.gt_val(&right),
            BinOp::Ge(_) => left.ge_val(&right),
            BinOp::And(_) => Ok(Value::Bool(left.as_bool()? && right.as_bool()?)),
            BinOp::Or(_) => Ok(Value::Bool(left.as_bool()? || right.as_bool()?)),
            BinOp::BitAnd(_) => left.bitand(&right),
            BinOp::BitOr(_) => left.bitor(&right),
            BinOp::BitXor(_) => left.bitxor(&right),
            BinOp::Shl(_) => left.shl(&right),
            BinOp::Shr(_) => left.shr(&right),
            _ => Err(CrustError::Runtime(format!("unsupported binary operator: {}", quote::quote!(#op)))),
        }
    }

    /// Evaluate a function call expression.
    fn eval_call(&self, call: &syn::ExprCall, env: &Rc<RefCell<Environment>>) -> Result<Value, CrustError> {
        let func = self.eval_expr(&call.func, env)?;
        let args: Result<Vec<Value>, _> = call.args.iter()
            .map(|a| self.eval_expr(a, env))
            .collect();
        let args = args?;

        match func {
            Value::Function { params, body, closure_env, .. } => {
                let fn_env = Rc::new(RefCell::new(Environment::with_parent(closure_env)));
                for (i, (name, _ty)) in params.iter().enumerate() {
                    let arg = args.get(i).cloned().unwrap_or(Value::Unit);
                    fn_env.borrow_mut().set(name.clone(), arg);
                }
                match self.exec_block(&body, &fn_env) {
                    Ok(val) => Ok(val),
                    Err(CrustError::Return(val)) => Ok(val),
                    Err(e) => Err(e),
                }
            }
            Value::BuiltinFn(f) => f(args),
            Value::StructDef { fields: def_fields } => {
                // Struct constructor call — shouldn't happen for named structs
                // but handle tuple structs
                let mut fields = std::collections::HashMap::new();
                for (i, val) in args.into_iter().enumerate() {
                    if i < def_fields.len() {
                        fields.insert(def_fields[i].0.clone(), val);
                    }
                }
                let name = "anonymous".to_string(); // Will be replaced by path
                Ok(Value::Struct { name, fields })
            }
            _ => Err(CrustError::Runtime(format!("not callable: {:?}", func))),
        }
    }

    /// Evaluate a method call.
    fn eval_method_call(&self, method: &syn::ExprMethodCall, env: &Rc<RefCell<Environment>>) -> Result<Value, CrustError> {
        let receiver = self.eval_expr(&method.receiver, env)?;
        let method_name = method.method.to_string();
        let args: Result<Vec<Value>, _> = method.args.iter()
            .map(|a| self.eval_expr(a, env))
            .collect();
        let args = args?;

        // Delegate to Value's method dispatch
        receiver.call_method(&method_name, args, self, env)
    }

    /// Evaluate a macro invocation.
    fn eval_macro(&self, mac: &syn::ExprMacro, env: &Rc<RefCell<Environment>>) -> Result<Value, CrustError> {
        let macro_name = mac.mac.path.segments.last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();

        let tokens = &mac.mac.tokens;
        crate::stdlib::eval_macro(&macro_name, tokens, self, env)
    }

    /// Assign to an expression target (variable, field, index).
    fn assign_expr(&self, target: &Expr, val: Value, env: &Rc<RefCell<Environment>>) -> Result<(), CrustError> {
        match target {
            Expr::Path(path) => {
                let name = path_to_string(&path.path);
                env.borrow_mut().update(&name, val)
                    .map_err(|_| CrustError::Runtime(format!("cannot assign to undefined variable: `{}`", name)))
            }
            Expr::Index(idx) => {
                let index = self.eval_expr(&idx.index, env)?;
                let i = index.as_i64()? as usize;
                // Get the vec and mutate in-place
                let obj = self.eval_expr(&idx.expr, env)?;
                if let Value::Vec(vec) = obj {
                    let mut v = vec.borrow_mut();
                    if i < v.len() {
                        v[i] = val;
                        Ok(())
                    } else {
                        Err(CrustError::Runtime(format!("index {} out of bounds (len {})", i, v.len())))
                    }
                } else {
                    Err(CrustError::Runtime("cannot index non-vec".into()))
                }
            }
            Expr::Field(field) => {
                let field_name = match &field.member {
                    syn::Member::Named(ident) => ident.to_string(),
                    syn::Member::Unnamed(idx) => idx.index.to_string(),
                };
                // In hack mode, we need to get the struct and update it
                let obj = self.eval_expr(&field.base, env)?;
                if let Value::Struct { name, mut fields } = obj {
                    fields.insert(field_name, val);
                    // Re-assign the struct back
                    self.assign_expr(&field.base, Value::Struct { name, fields }, env)
                } else {
                    Err(CrustError::Runtime("cannot set field on non-struct".into()))
                }
            }
            _ => Err(CrustError::Runtime("invalid assignment target".into())),
        }
    }

    /// Try to match a value against a match arm. Returns Some(value) if matched.
    fn try_match_arm(&self, scrutinee: &Value, arm: &syn::Arm, env: &Rc<RefCell<Environment>>) -> Result<Option<Value>, CrustError> {
        let arm_env = Rc::new(RefCell::new(Environment::with_parent(env.clone())));

        if self.pattern_matches(scrutinee, &arm.pat, &arm_env)? {
            // Check guard
            if let Some((_, guard)) = &arm.guard {
                let guard_val = self.eval_expr(guard, &arm_env)?;
                if !guard_val.as_bool()? {
                    return Ok(None);
                }
            }
            let val = self.eval_expr(&arm.body, &arm_env)?;
            Ok(Some(val))
        } else {
            Ok(None)
        }
    }

    /// Check if a value matches a pattern, binding variables if so.
    fn pattern_matches(&self, val: &Value, pat: &Pat, env: &Rc<RefCell<Environment>>) -> Result<bool, CrustError> {
        match pat {
            Pat::Wild(_) => Ok(true),
            Pat::Ident(ident) => {
                env.borrow_mut().set(ident.ident.to_string(), val.clone());
                Ok(true)
            }
            Pat::Lit(lit_pat) => {
                let lit_val = self.eval_lit(&lit_pat.lit)?;
                Ok(*val == lit_val)
            }
            Pat::Or(or_pat) => {
                for p in &or_pat.cases {
                    if self.pattern_matches(val, p, env)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Pat::Tuple(tuple) => {
                if let Value::Tuple(vals) = val {
                    if vals.len() != tuple.elems.len() {
                        return Ok(false);
                    }
                    for (v, p) in vals.iter().zip(tuple.elems.iter()) {
                        if !self.pattern_matches(v, p, env)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Pat::Range(range_pat) => {
                let val_i = val.as_i64()?;
                let lo = if let Some(lo) = &range_pat.start {
                    self.eval_expr(lo, env)?.as_i64()?
                } else {
                    i64::MIN
                };
                let hi = if let Some(hi) = &range_pat.end {
                    self.eval_expr(hi, env)?.as_i64()?
                } else {
                    i64::MAX
                };
                match &range_pat.limits {
                    syn::RangeLimits::HalfOpen(_) => Ok(val_i >= lo && val_i < hi),
                    syn::RangeLimits::Closed(_) => Ok(val_i >= lo && val_i <= hi),
                }
            }
            _ => Ok(false),
        }
    }
}

// ── Helper functions ──────────────────────────────────────────────────

fn pat_to_string(pat: &Pat) -> String {
    match pat {
        Pat::Ident(ident) => ident.ident.to_string(),
        Pat::Type(pt) => pat_to_string(&pt.pat),
        _ => format!("{}", quote::quote!(#pat)),
    }
}

fn type_to_string(ty: &syn::Type) -> String {
    format!("{}", quote::quote!(#ty))
}

fn path_to_string(path: &syn::Path) -> String {
    path.segments.iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}
