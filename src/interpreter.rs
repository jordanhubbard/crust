use crate::ast::*;
use crate::environment::Environment;
use crate::value::Value;
/// Tree-walk interpreter for Crust Level 0.
use std::collections::HashMap;

pub struct Interpreter {
    pub env: Environment,
    /// Struct definitions: name → ordered field names.
    pub struct_defs: HashMap<String, Vec<String>>,
}

/// Control-flow signal returned by statement execution.
enum Signal {
    None,
    Break,
    Return(Value),
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            env: Environment::new(),
            struct_defs: HashMap::new(),
        }
    }

    // ----------------------------------------------------------------
    // Public entry points
    // ----------------------------------------------------------------

    pub fn run(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        for stmt in stmts {
            match self.exec_stmt(stmt)? {
                Signal::Break => return Err("error: `break` outside of a loop".to_string()),
                Signal::Return(_) => return Ok(()),
                Signal::None => {}
            }
        }
        Ok(())
    }

    pub fn run_expr(&mut self, stmts: &[Stmt]) -> Result<Option<Value>, String> {
        let mut last = None;
        for stmt in stmts {
            match stmt {
                Stmt::ExprStmt(expr) => {
                    // Eval once; capture for REPL display. No exec_stmt double-eval.
                    last = Some(self.eval_expr(expr)?);
                }
                _ => match self.exec_stmt(stmt)? {
                    Signal::Break => return Err("error: `break` outside of a loop".to_string()),
                    Signal::Return(v) => return Ok(Some(v)),
                    Signal::None => {}
                },
            }
        }
        Ok(last)
    }

    // ----------------------------------------------------------------
    // Statement execution
    // ----------------------------------------------------------------

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<Signal, String> {
        match stmt {
            Stmt::Let { name, value } => {
                let val = self.eval_expr(value)?;
                self.env.define(name.clone(), val);
                Ok(Signal::None)
            }

            Stmt::StructDef { name, fields } => {
                self.struct_defs.insert(
                    name.clone(),
                    fields.iter().map(|(n, _)| n.clone()).collect(),
                );
                Ok(Signal::None)
            }

            Stmt::FnDef { name, params, body } => {
                let val = Value::Fn {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                };
                self.env.define(name.clone(), val);
                Ok(Signal::None)
            }

            Stmt::ExprStmt(expr) => {
                self.eval_expr(expr)?;
                Ok(Signal::None)
            }

            Stmt::While { condition, body } => {
                loop {
                    if !self.eval_expr(condition)?.is_truthy() {
                        break;
                    }
                    match self.exec_block(body)? {
                        Signal::Break => break,
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::None => {}
                    }
                }
                Ok(Signal::None)
            }

            Stmt::Loop { body } => {
                loop {
                    match self.exec_block(body)? {
                        Signal::Break => break,
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::None => {}
                    }
                }
                Ok(Signal::None)
            }

            Stmt::For { var, iter, body } => {
                let items = self.eval_iter(iter)?;
                for item in items {
                    self.env.push_scope();
                    self.env.define(var.clone(), item);
                    let sig = self.exec_stmts(body)?;
                    self.env.pop_scope();
                    match sig {
                        Signal::Break => break,
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::None => {}
                    }
                }
                Ok(Signal::None)
            }

            Stmt::Break => Ok(Signal::Break),

            Stmt::Return(expr) => {
                let val = match expr {
                    Some(e) => self.eval_expr(e)?,
                    None => Value::Unit,
                };
                Ok(Signal::Return(val))
            }

            Stmt::Assign { target, value } => {
                let val = self.eval_expr(value)?;
                self.exec_assign(target, val)?;
                Ok(Signal::None)
            }
        }
    }

    fn exec_block(&mut self, stmts: &[Stmt]) -> Result<Signal, String> {
        self.env.push_scope();
        let sig = self.exec_stmts(stmts);
        self.env.pop_scope();
        sig
    }

    fn exec_stmts(&mut self, stmts: &[Stmt]) -> Result<Signal, String> {
        for stmt in stmts {
            match self.exec_stmt(stmt)? {
                Signal::None => {}
                sig => return Ok(sig),
            }
        }
        Ok(Signal::None)
    }

    /// Evaluate a block and return the value of its last expression (no double-eval).
    fn eval_block_value(&mut self, stmts: &[Stmt]) -> Result<Value, String> {
        let mut last = Value::Unit;
        for stmt in stmts {
            match stmt {
                Stmt::ExprStmt(e) => {
                    // Eval once, capture value directly — avoids double-evaluation.
                    last = self.eval_expr(e)?;
                }
                _ => match self.exec_stmt(stmt)? {
                    Signal::Return(v) => return Ok(v),
                    Signal::Break => return Err("break in block expression".into()),
                    Signal::None => {
                        last = Value::Unit;
                    }
                },
            }
        }
        Ok(last)
    }

    // ----------------------------------------------------------------
    // Iteration
    // ----------------------------------------------------------------

    fn eval_iter(&mut self, expr: &Expr) -> Result<Vec<Value>, String> {
        match expr {
            Expr::Range {
                start,
                end,
                inclusive,
            } => {
                let s = self.eval_expr(start)?;
                let e = self.eval_expr(end)?;
                match (s, e) {
                    (Value::Int(s), Value::Int(e)) => {
                        let end_val = if *inclusive { e + 1 } else { e };
                        Ok((s..end_val).map(Value::Int).collect())
                    }
                    (a, b) => Err(format!(
                        "range requires integer bounds, got {} and {}",
                        a.type_name(),
                        b.type_name()
                    )),
                }
            }
            _ => match self.eval_expr(expr)? {
                Value::Vec(items) => Ok(items),
                Value::String(s) => Ok(s.chars().map(|c| Value::String(c.to_string())).collect()),
                v => Err(format!("cannot iterate over {}", v.type_name())),
            },
        }
    }

    // ----------------------------------------------------------------
    // Assignment
    // ----------------------------------------------------------------

    fn exec_assign(&mut self, target: &AssignTarget, val: Value) -> Result<(), String> {
        match target {
            AssignTarget::Variable(name) => self.env.set(name, val),

            AssignTarget::Field(obj_expr, field) => {
                let var_name = expr_ident(obj_expr)
                    .ok_or("only simple variable field assignment is supported")?;
                let mut obj = self
                    .env
                    .get(&var_name)
                    .ok_or_else(|| format!("undefined variable '{}'", var_name))?;
                match &mut obj {
                    Value::Struct { fields, .. } => {
                        if let Some(f) = fields.iter_mut().find(|(n, _)| n == field) {
                            f.1 = val;
                        } else {
                            fields.push((field.clone(), val));
                        }
                    }
                    _ => return Err(format!("'{}' is not a struct", var_name)),
                }
                self.env.set(&var_name, obj)
            }

            AssignTarget::Index(obj_expr, idx_expr) => {
                let var_name = expr_ident(obj_expr)
                    .ok_or("only simple variable index assignment is supported")?;
                let idx = self.eval_expr(idx_expr)?;
                let mut obj = self
                    .env
                    .get(&var_name)
                    .ok_or_else(|| format!("undefined variable '{}'", var_name))?;
                match &mut obj {
                    Value::Vec(items) => {
                        let i = int_index(&idx, items.len())?;
                        items[i] = val;
                    }
                    _ => return Err(format!("'{}' is not indexable", var_name)),
                }
                self.env.set(&var_name, obj)
            }
        }
    }

    // ----------------------------------------------------------------
    // Expression evaluation
    // ----------------------------------------------------------------

    pub fn eval_expr(&mut self, expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::IntLit(n) => Ok(Value::Int(*n)),
            Expr::FloatLit(f) => Ok(Value::Float(*f)),
            Expr::BoolLit(b) => Ok(Value::Bool(*b)),
            Expr::StringLit(s) => Ok(Value::String(s.clone())),

            Expr::Ident(name) => self
                .env
                .get(name)
                .ok_or_else(|| format!("undefined variable '{}'", name)),

            Expr::BinaryOp { left, op, right } => self.eval_binop(left, *op, right),

            Expr::UnaryOp { op, expr } => {
                let v = self.eval_expr(expr)?;
                match op {
                    UnaryOp::Neg => match v {
                        Value::Int(n) => Ok(Value::Int(-n)),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        _ => Err(format!("cannot negate {}", v.type_name())),
                    },
                    UnaryOp::Not => Ok(Value::Bool(!v.is_truthy())),
                }
            }

            Expr::PrintLn {
                format_str,
                args,
                newline,
            } => {
                let out = self.format_string(format_str, args)?;
                if *newline {
                    println!("{}", out);
                } else {
                    print!("{}", out);
                }
                Ok(Value::Unit)
            }

            Expr::VecLit(elems) => {
                let vals: Result<Vec<Value>, _> = elems.iter().map(|e| self.eval_expr(e)).collect();
                Ok(Value::Vec(vals?))
            }

            Expr::StructInit { name, fields } => {
                let mut fv = Vec::new();
                for (fname, fexpr) in fields {
                    fv.push((fname.clone(), self.eval_expr(fexpr)?));
                }
                Ok(Value::Struct {
                    name: name.clone(),
                    fields: fv,
                })
            }

            Expr::FieldAccess { object, field } => {
                let obj = self.eval_expr(object)?;
                match obj {
                    Value::Struct { fields, .. } => fields
                        .into_iter()
                        .find(|(n, _)| n == field)
                        .map(|(_, v)| v)
                        .ok_or_else(|| format!("no field '{}' on struct", field)),
                    _ => Err(format!(
                        "cannot access field '{}' on {}",
                        field,
                        obj.type_name()
                    )),
                }
            }

            Expr::IndexAccess { object, index } => {
                let obj = self.eval_expr(object)?;
                let idx = self.eval_expr(index)?;
                match obj {
                    Value::Vec(items) => {
                        let i = int_index(&idx, items.len())?;
                        Ok(items[i].crust_clone())
                    }
                    Value::String(s) => {
                        let chars: Vec<char> = s.chars().collect();
                        let i = int_index(&idx, chars.len())?;
                        Ok(Value::String(chars[i].to_string()))
                    }
                    v => Err(format!("cannot index into {}", v.type_name())),
                }
            }

            Expr::MethodCall {
                object,
                method,
                args,
            } => self.eval_method_call(object, method, args),

            Expr::Call { function, args } => self.eval_call(function, args),

            Expr::If {
                condition,
                then_block,
                else_block,
            } => {
                let cond = self.eval_expr(condition)?;
                let block = if cond.is_truthy() {
                    Some(then_block.as_slice())
                } else {
                    else_block.as_deref()
                };
                if let Some(b) = block {
                    self.env.push_scope();
                    let last = self.eval_block_value(b)?;
                    self.env.pop_scope();
                    Ok(last)
                } else {
                    Ok(Value::Unit)
                }
            }

            Expr::Block(stmts) => {
                self.env.push_scope();
                let last = self.eval_block_value(stmts)?;
                self.env.pop_scope();
                Ok(last)
            }

            Expr::Range { .. } => {
                let items = self.eval_iter(expr)?;
                Ok(Value::Vec(items))
            }
        }
    }

    // ----------------------------------------------------------------
    // Binary operations
    // ----------------------------------------------------------------

    fn eval_binop(&mut self, left: &Expr, op: BinOp, right: &Expr) -> Result<Value, String> {
        // Short-circuit for logical ops
        if op == BinOp::And {
            let l = self.eval_expr(left)?;
            if !l.is_truthy() {
                return Ok(Value::Bool(false));
            }
            return Ok(Value::Bool(self.eval_expr(right)?.is_truthy()));
        }
        if op == BinOp::Or {
            let l = self.eval_expr(left)?;
            if l.is_truthy() {
                return Ok(Value::Bool(true));
            }
            return Ok(Value::Bool(self.eval_expr(right)?.is_truthy()));
        }

        let lv = self.eval_expr(left)?;
        let rv = self.eval_expr(right)?;

        match op {
            BinOp::Add => match (lv, rv) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + b as f64)),
                // String concat: anything can be appended with +
                (Value::String(a), b) => Ok(Value::String(a + &b.display_fmt())),
                (a, Value::String(b)) => Ok(Value::String(a.display_fmt() + &b)),
                (a, b) => Err(format!(
                    "cannot add {} and {}",
                    a.type_name(),
                    b.type_name()
                )),
            },
            BinOp::Sub => num_binop(lv, rv, "-", |a, b| a - b, |a, b| a - b),
            BinOp::Mul => num_binop(lv, rv, "*", |a, b| a * b, |a, b| a * b),
            BinOp::Div => {
                match (&lv, &rv) {
                    (Value::Int(_), Value::Int(0)) | (Value::Float(_), Value::Int(0)) => {
                        return Err("division by zero".to_string());
                    }
                    _ => {}
                }
                num_binop(lv, rv, "/", |a, b| a / b, |a, b| a / b)
            }
            BinOp::Mod => match (lv, rv) {
                (Value::Int(a), Value::Int(b)) => {
                    if b == 0 {
                        return Err("modulo by zero".to_string());
                    }
                    Ok(Value::Int(a % b))
                }
                (a, b) => Err(format!(
                    "cannot mod {} and {}",
                    a.type_name(),
                    b.type_name()
                )),
            },
            BinOp::Eq => Ok(Value::Bool(val_eq(&lv, &rv))),
            BinOp::NotEq => Ok(Value::Bool(!val_eq(&lv, &rv))),
            BinOp::Lt => cmp_op(&lv, &rv, std::cmp::Ordering::Less, false),
            BinOp::Gt => cmp_op(&lv, &rv, std::cmp::Ordering::Greater, false),
            BinOp::LtEq => cmp_op(&lv, &rv, std::cmp::Ordering::Greater, true),
            BinOp::GtEq => cmp_op(&lv, &rv, std::cmp::Ordering::Less, true),
            BinOp::And | BinOp::Or => unreachable!(),
        }
    }

    // ----------------------------------------------------------------
    // Method calls
    // ----------------------------------------------------------------

    fn eval_method_call(
        &mut self,
        obj_expr: &Expr,
        method: &str,
        args: &[Expr],
    ) -> Result<Value, String> {
        // Mutating methods need to update the variable in place.
        const MUTATING: &[&str] = &[
            "push",
            "pop",
            "clear",
            "sort",
            "sort_by_key",
            "reverse",
            "dedup",
            "retain",
            "truncate",
            "insert",
            "remove",
            "drain",
        ];
        let is_mutating = MUTATING.contains(&method);

        if is_mutating {
            if let Some(var_name) = expr_ident(obj_expr) {
                let mut obj = self
                    .env
                    .get(&var_name)
                    .ok_or_else(|| format!("undefined variable '{}'", var_name))?;
                let result = self.apply_mutating_method(&mut obj, method, args)?;
                self.env.set(&var_name, obj)?;
                return Ok(result);
            }
        }

        let obj = self.eval_expr(obj_expr)?;
        self.apply_method(obj, method, args)
    }

    fn apply_mutating_method(
        &mut self,
        obj: &mut Value,
        method: &str,
        args: &[Expr],
    ) -> Result<Value, String> {
        match obj {
            Value::Vec(items) => match method {
                "push" => {
                    let v = self.eval_expr(args.first().ok_or("push() needs 1 arg")?)?;
                    items.push(v);
                    Ok(Value::Unit)
                }
                "pop" => Ok(items.pop().unwrap_or(Value::Unit)),
                "clear" => {
                    items.clear();
                    Ok(Value::Unit)
                }
                "sort" => {
                    items.sort_by(|a, b| val_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal));
                    Ok(Value::Unit)
                }
                "reverse" => {
                    items.reverse();
                    Ok(Value::Unit)
                }
                "dedup" => {
                    items.dedup_by(|a, b| val_eq(a, b));
                    Ok(Value::Unit)
                }
                "insert" => {
                    if args.len() != 2 {
                        return Err("insert(idx, val) needs 2 args".into());
                    }
                    let idx = int_index(&self.eval_expr(&args[0])?, items.len() + 1)?;
                    let v = self.eval_expr(&args[1])?;
                    items.insert(idx, v);
                    Ok(Value::Unit)
                }
                "remove" => {
                    if args.is_empty() {
                        return Err("remove(idx) needs 1 arg".into());
                    }
                    let idx = int_index(&self.eval_expr(&args[0])?, items.len())?;
                    Ok(items.remove(idx))
                }
                "truncate" => {
                    if args.is_empty() {
                        return Err("truncate(n) needs 1 arg".into());
                    }
                    let n = int_index(&self.eval_expr(&args[0])?, usize::MAX)?;
                    items.truncate(n);
                    Ok(Value::Unit)
                }
                m => Err(format!("unknown mutating method '{}' on Vec", m)),
            },
            _ => Err(format!(
                "method '{}' not applicable to {}",
                method,
                obj.type_name()
            )),
        }
    }

    fn apply_method(&mut self, obj: Value, method: &str, args: &[Expr]) -> Result<Value, String> {
        match &obj {
            Value::Vec(items) => match method {
                "len" => Ok(Value::Int(items.len() as i64)),
                "is_empty" => Ok(Value::Bool(items.is_empty())),
                "clone" | "iter" | "into_iter" => Ok(obj.crust_clone()),
                "first" => Ok(items.first().cloned().unwrap_or(Value::Unit)),
                "last" => Ok(items.last().cloned().unwrap_or(Value::Unit)),
                "contains" => {
                    let t = self.eval_expr(one_arg(args, "contains")?)?;
                    Ok(Value::Bool(items.iter().any(|v| val_eq(v, &t))))
                }
                "join" => {
                    let sep = self.eval_expr(one_arg(args, "join")?)?.display_fmt();
                    let parts: Vec<String> = items.iter().map(|v| v.display_fmt()).collect();
                    Ok(Value::String(parts.join(&sep)))
                }
                "get" => {
                    let idx = int_index(&self.eval_expr(one_arg(args, "get")?)?, items.len())?;
                    Ok(items.get(idx).cloned().unwrap_or(Value::Unit))
                }
                m => Err(format!("no method '{}' on Vec", m)),
            },

            Value::String(s) => match method {
                "len" => Ok(Value::Int(s.len() as i64)),
                "is_empty" => Ok(Value::Bool(s.is_empty())),
                "to_uppercase" => Ok(Value::String(s.to_uppercase())),
                "to_lowercase" => Ok(Value::String(s.to_lowercase())),
                "trim" => Ok(Value::String(s.trim().to_string())),
                "trim_start" | "trim_left" => Ok(Value::String(s.trim_start().to_string())),
                "trim_end" | "trim_right" => Ok(Value::String(s.trim_end().to_string())),
                "clone" | "to_string" => Ok(obj.crust_clone()),
                "chars" => Ok(Value::Vec(
                    s.chars().map(|c| Value::String(c.to_string())).collect(),
                )),
                "bytes" => Ok(Value::Vec(
                    s.bytes().map(|b| Value::Int(b as i64)).collect(),
                )),
                "lines" => Ok(Value::Vec(
                    s.lines().map(|l| Value::String(l.to_string())).collect(),
                )),
                "contains" => {
                    let pat = self.eval_expr(one_arg(args, "contains")?)?.display_fmt();
                    Ok(Value::Bool(s.contains(pat.as_str())))
                }
                "starts_with" => {
                    let pat = self.eval_expr(one_arg(args, "starts_with")?)?.display_fmt();
                    Ok(Value::Bool(s.starts_with(pat.as_str())))
                }
                "ends_with" => {
                    let pat = self.eval_expr(one_arg(args, "ends_with")?)?.display_fmt();
                    Ok(Value::Bool(s.ends_with(pat.as_str())))
                }
                "replace" => {
                    if args.len() < 2 {
                        return Err("replace(from, to) needs 2 args".into());
                    }
                    let from = self.eval_expr(&args[0])?.display_fmt();
                    let to = self.eval_expr(&args[1])?.display_fmt();
                    Ok(Value::String(s.replace(from.as_str(), to.as_str())))
                }
                "split" => {
                    let sep = self.eval_expr(one_arg(args, "split")?)?.display_fmt();
                    Ok(Value::Vec(
                        s.split(sep.as_str())
                            .map(|p| Value::String(p.to_string()))
                            .collect(),
                    ))
                }
                "repeat" => {
                    let n = match self.eval_expr(one_arg(args, "repeat")?)? {
                        Value::Int(n) => n as usize,
                        v => return Err(format!("repeat() needs integer, got {}", v.type_name())),
                    };
                    Ok(Value::String(s.repeat(n)))
                }
                "parse" => {
                    if let Ok(n) = s.parse::<i64>() {
                        return Ok(Value::Int(n));
                    }
                    if let Ok(f) = s.parse::<f64>() {
                        return Ok(Value::Float(f));
                    }
                    Err(format!("cannot parse '{}' as a number", s))
                }
                m => Err(format!("no method '{}' on String", m)),
            },

            Value::Int(n) => match method {
                "to_string" => Ok(Value::String(n.to_string())),
                "abs" => Ok(Value::Int(n.abs())),
                "pow" => {
                    let e = match self.eval_expr(one_arg(args, "pow")?)? {
                        Value::Int(e) => e as u32,
                        v => {
                            return Err(format!(
                                "pow() needs integer exponent, got {}",
                                v.type_name()
                            ))
                        }
                    };
                    Ok(Value::Int(n.pow(e)))
                }
                "min" => match self.eval_expr(one_arg(args, "min")?)? {
                    Value::Int(b) => Ok(Value::Int((*n).min(b))),
                    v => Err(format!("min() needs Int, got {}", v.type_name())),
                },
                "max" => match self.eval_expr(one_arg(args, "max")?)? {
                    Value::Int(b) => Ok(Value::Int((*n).max(b))),
                    v => Err(format!("max() needs Int, got {}", v.type_name())),
                },
                m => Err(format!("no method '{}' on Int", m)),
            },

            Value::Float(f) => match method {
                "to_string" => Ok(Value::String(f.to_string())),
                "abs" => Ok(Value::Float(f.abs())),
                "sqrt" => Ok(Value::Float(f.sqrt())),
                "floor" => Ok(Value::Float(f.floor())),
                "ceil" => Ok(Value::Float(f.ceil())),
                "round" => Ok(Value::Float(f.round())),
                "sin" => Ok(Value::Float(f.sin())),
                "cos" => Ok(Value::Float(f.cos())),
                "tan" => Ok(Value::Float(f.tan())),
                "ln" => Ok(Value::Float(f.ln())),
                "log2" => Ok(Value::Float(f.log2())),
                "powi" => {
                    let e = match self.eval_expr(one_arg(args, "powi")?)? {
                        Value::Int(e) => e as i32,
                        v => {
                            return Err(format!(
                                "powi() needs integer exponent, got {}",
                                v.type_name()
                            ))
                        }
                    };
                    Ok(Value::Float(f.powi(e)))
                }
                "powf" => {
                    let e = match self.eval_expr(one_arg(args, "powf")?)? {
                        Value::Float(e) => e,
                        Value::Int(e) => e as f64,
                        v => {
                            return Err(format!(
                                "powf() needs float exponent, got {}",
                                v.type_name()
                            ))
                        }
                    };
                    Ok(Value::Float(f.powf(e)))
                }
                m => Err(format!("no method '{}' on Float", m)),
            },

            _ => Err(format!("no method '{}' on {}", method, obj.type_name())),
        }
    }

    // ----------------------------------------------------------------
    // Function calls
    // ----------------------------------------------------------------

    fn eval_call(&mut self, function: &Expr, args: &[Expr]) -> Result<Value, String> {
        // Intercept builtin pseudo-functions
        if let Expr::Ident(name) = function {
            match name.as_str() {
                "__builtin_panic" | "__builtin_todo" | "__builtin_unimplemented" => {
                    let msg = if !args.is_empty() {
                        self.eval_expr(&args[0])?.display_fmt()
                    } else {
                        name.strip_prefix("__builtin_").unwrap_or(name).to_string()
                    };
                    return Err(format!("explicit panic: {}", msg));
                }
                "__builtin_format" => {
                    if args.is_empty() {
                        return Err("format! needs a format string".into());
                    }
                    let fmt = match self.eval_expr(&args[0])? {
                        Value::String(s) => s,
                        v => v.display_fmt(),
                    };
                    let rest = &args[1..];
                    let out = self.format_string(&fmt, rest)?;
                    return Ok(Value::String(out));
                }
                _ => {}
            }
        }

        let fn_val = self.eval_expr(function)?;
        match fn_val {
            Value::Fn { params, body, .. } => {
                if args.len() != params.len() {
                    return Err(format!(
                        "function expects {} arguments, got {}",
                        params.len(),
                        args.len()
                    ));
                }
                // Evaluate args in current scope before pushing new one
                let mut arg_vals = Vec::with_capacity(args.len());
                for arg in args {
                    arg_vals.push(self.eval_expr(arg)?);
                }

                self.env.push_scope();
                for ((pname, _), val) in params.iter().zip(arg_vals) {
                    self.env.define(pname.clone(), val);
                }
                let mut result = Value::Unit;
                for stmt in &body.clone() {
                    match self.exec_stmt(stmt)? {
                        Signal::Return(v) => {
                            result = v;
                            break;
                        }
                        Signal::Break => {
                            self.env.pop_scope();
                            return Err("break outside loop inside function".into());
                        }
                        Signal::None => {}
                    }
                }
                self.env.pop_scope();
                Ok(result)
            }
            v => Err(format!("'{}' is not a function", v.display_fmt())),
        }
    }

    // ----------------------------------------------------------------
    // Format strings
    // ----------------------------------------------------------------

    pub fn format_string(&mut self, fmt: &str, args: &[Expr]) -> Result<String, String> {
        let mut result = String::new();
        let mut chars = fmt.chars().peekable();
        let mut pos_idx = 0usize;

        while let Some(c) = chars.next() {
            if c != '{' && c != '}' {
                result.push(c);
                continue;
            }
            if c == '}' {
                if chars.peek() == Some(&'}') {
                    chars.next();
                    result.push('}');
                } else {
                    return Err("unexpected '}' in format string".into());
                }
                continue;
            }
            // c == '{'
            if chars.peek() == Some(&'{') {
                chars.next();
                result.push('{');
                continue;
            }

            // Collect spec until '}'
            let mut spec = String::new();
            for ch in chars.by_ref() {
                if ch == '}' {
                    break;
                }
                spec.push(ch);
            }

            let (var_part, fmt_spec) = if let Some(colon) = spec.find(':') {
                (&spec[..colon], &spec[colon + 1..])
            } else {
                (spec.as_str(), "")
            };

            let debug = fmt_spec == "?" || fmt_spec.ends_with('?');

            let val = if var_part.is_empty() {
                // Next positional arg
                if pos_idx >= args.len() {
                    return Err("not enough arguments for format string".into());
                }
                let v = self.eval_expr(&args[pos_idx])?;
                pos_idx += 1;
                v
            } else if let Ok(idx) = var_part.parse::<usize>() {
                if idx >= args.len() {
                    return Err(format!("format arg index {} out of range", idx));
                }
                self.eval_expr(&args[idx])?
            } else {
                // Inline variable name
                self.env
                    .get(var_part)
                    .ok_or_else(|| format!("undefined variable '{}' in format string", var_part))?
            };

            if debug {
                result.push_str(&val.debug_fmt());
            } else {
                result.push_str(&val.display_fmt());
            }
        }
        Ok(result)
    }
}

// ----------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------

fn expr_ident(expr: &Expr) -> Option<String> {
    if let Expr::Ident(name) = expr {
        Some(name.clone())
    } else {
        None
    }
}

fn one_arg<'a>(args: &'a [Expr], method: &str) -> Result<&'a Expr, String> {
    args.first()
        .ok_or_else(|| format!("{}() requires 1 argument", method))
}

fn int_index(idx: &Value, len: usize) -> Result<usize, String> {
    match idx {
        Value::Int(n) => {
            let n = *n;
            if n < 0 {
                return Err(format!("negative index {}", n));
            }
            let u = n as usize;
            if u >= len && len != usize::MAX {
                return Err(format!("index {} out of bounds (len={})", n, len));
            }
            Ok(u)
        }
        v => Err(format!("index must be Int, got {}", v.type_name())),
    }
}

fn val_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Int(x), Value::Float(y)) => (*x as f64) == *y,
        (Value::Float(x), Value::Int(y)) => *x == (*y as f64),
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Unit, Value::Unit) => true,
        (Value::Vec(x), Value::Vec(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| val_eq(a, b))
        }
        _ => false,
    }
}

fn val_cmp(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y),
        (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

/// `target` is the ordering we're testing for; `negate` inverts (for <=, >=).
/// Lt  → (target=Less,    negate=false) → ord == Less
/// Gt  → (target=Greater, negate=false) → ord == Greater
/// LtEq→ (target=Greater, negate=true)  → ord != Greater
/// GtEq→ (target=Less,    negate=true)  → ord != Less
fn cmp_op(a: &Value, b: &Value, target: std::cmp::Ordering, negate: bool) -> Result<Value, String> {
    match val_cmp(a, b) {
        Some(ord) => Ok(Value::Bool(if negate {
            ord != target
        } else {
            ord == target
        })),
        None => Err(format!(
            "cannot compare {} and {}",
            a.type_name(),
            b.type_name()
        )),
    }
}

fn num_binop(
    lv: Value,
    rv: Value,
    op: &str,
    int_op: fn(i64, i64) -> i64,
    float_op: fn(f64, f64) -> f64,
) -> Result<Value, String> {
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(int_op(a, b))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(a, b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_op(a as f64, b))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_op(a, b as f64))),
        (a, b) => Err(format!(
            "cannot apply '{}' to {} and {}",
            op,
            a.type_name(),
            b.type_name()
        )),
    }
}
