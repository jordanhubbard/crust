use crate::ast::*;
use crate::strictness::StrictnessLevel;

pub struct Codegen {
    indent: usize,
    /// The strictness level controls which safety features and annotations are emitted.
    pub level: StrictnessLevel,
    /// When true, ownership-transfer comments are injected at every clone/move site,
    /// making LLM-generated code auditable by human reviewers.
    pub llm_mode: bool,
}

impl Codegen {
    /// Kept for backward compatibility and direct test usage.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Codegen { indent: 0, level: StrictnessLevel::Explore, llm_mode: false }
    }

    pub fn with_level(level: StrictnessLevel) -> Self {
        Codegen { indent: 0, level, llm_mode: false }
    }

    pub fn emit_program(&mut self, items: &[Item]) -> String {
        let mut out = String::new();
        out.push_str("use std::collections::HashMap;\n\n");
        for item in items {
            out.push_str(&self.emit_item(item));
            out.push('\n');
        }
        out
    }

    fn indent_str(&self) -> String {
        "    ".repeat(self.indent)
    }

    fn emit_item(&mut self, item: &Item) -> String {
        match item {
            Item::Fn(f) => self.emit_fn(f),
            Item::Struct(s) => self.emit_struct(s),
            Item::Enum(e) => self.emit_enum(e),
            Item::Impl(i) => self.emit_impl(i),
            Item::Use(path) => format!("use {};\n", path.join("::")),
            Item::Const { name, ty, value } => {
                format!("const {}: {} = {};\n", name, self.emit_ty(ty), self.emit_expr(value))
            }
            Item::TypeAlias { name, ty } => {
                format!("type {} = {};\n", name, self.emit_ty(ty))
            }
            Item::Trait { name, methods } => {
                let mut out = format!("trait {} {{\n", name);
                for m in methods {
                    out.push_str(&format!("    {}", self.emit_fn(m)));
                }
                out.push_str("}\n");
                out
            }
        }
    }

    fn emit_struct(&mut self, s: &StructDef) -> String {
        let mut out = String::new();
        // Level 0-2: auto-derive common traits so beginners don't hit E0277.
        // Level 3+: no implicit derives; the developer controls the derive list.
        if self.level < StrictnessLevel::Ship {
            out.push_str("#[derive(Clone, Debug, PartialEq)]\n");
        }
        out.push_str(&format!("struct {} {{\n", s.name));
        for (name, ty) in &s.fields {
            out.push_str(&format!("    {}: {},\n", name, self.emit_ty(ty)));
        }
        out.push_str("}\n");
        out
    }

    fn emit_enum(&mut self, e: &EnumDef) -> String {
        let mut out = String::new();
        if self.level < StrictnessLevel::Ship {
            out.push_str("#[derive(Clone, Debug, PartialEq)]\n");
        }
        out.push_str(&format!("enum {} {{\n", e.name));
        for v in &e.variants {
            out.push_str("    ");
            out.push_str(&v.name);
            match &v.data {
                VariantData::Unit => out.push_str(",\n"),
                VariantData::Tuple(tys) => {
                    out.push('(');
                    out.push_str(&tys.iter().map(|t| self.emit_ty(t)).collect::<Vec<_>>().join(", "));
                    out.push_str("),\n");
                }
                VariantData::Struct(fields) => {
                    out.push_str(" {\n");
                    for (n, t) in fields {
                        out.push_str(&format!("        {}: {},\n", n, self.emit_ty(t)));
                    }
                    out.push_str("    },\n");
                }
            }
        }
        out.push_str("}\n");
        out
    }

    fn emit_impl(&mut self, i: &ImplDef) -> String {
        let mut out = String::new();
        if let Some(tr) = &i.trait_name {
            out.push_str(&format!("impl {} for {} {{\n", tr, i.type_name));
        } else {
            out.push_str(&format!("impl {} {{\n", i.type_name));
        }
        self.indent += 1;
        for (name, expr) in &i.consts {
            out.push_str(&format!("{}const {}: _ = {};\n", "    ".repeat(self.indent), name, self.emit_expr(expr)));
        }
        for m in &i.methods {
            out.push_str(&self.emit_fn(m));
        }
        self.indent -= 1;
        out.push_str("}\n");
        out
    }

    fn emit_fn(&mut self, f: &FnDef) -> String {
        let ind = self.indent_str();

        // Emit attributes.  At Level 3+, re-emit all unknown attrs (e.g. derive, allow).
        // Crust-specific attrs (requires/ensures/invariant/pure) are emitted as comments
        // so the generated Rust is still valid.
        let mut out = String::new();
        for attr in &f.attrs {
            match attr {
                Attr::Pure =>
                    out.push_str(&format!("{}// #[pure] — verified side-effect-free by Crust\n", ind)),
                Attr::Requires(expr) =>
                    out.push_str(&format!("{}// #[requires({})]\n", ind, self.emit_expr(expr))),
                Attr::Ensures(expr) =>
                    out.push_str(&format!("{}// #[ensures({})]\n", ind, self.emit_expr(expr))),
                Attr::Invariant(expr) =>
                    out.push_str(&format!("{}// #[invariant({})]\n", ind, self.emit_expr(expr))),
                Attr::Unknown(s) if self.level >= StrictnessLevel::Ship =>
                    out.push_str(&format!("{}#[{}]\n", ind, s)),
                Attr::Unknown(_) => {} // lower levels: skip non-crust attrs (already derived)
            }
        }

        let params = f.params.iter().map(|p| {
            if p.is_self {
                "&self".to_string()
            } else if p.mutable {
                format!("mut {}: {}", p.name, self.emit_ty(&p.ty))
            } else {
                format!("{}: {}", p.name, self.emit_ty(&p.ty))
            }
        }).collect::<Vec<_>>().join(", ");

        let ret = if let Some(ty) = &f.ret_ty {
            format!(" -> {}", self.emit_ty(ty))
        } else {
            String::new()
        };

        let async_kw = if f.is_async { "async " } else { "" };
        out.push_str(&format!("{}{}fn {}({}){} {{\n", ind, async_kw, f.name, params, ret));
        self.indent += 1;
        for stmt in &f.body.stmts {
            out.push_str(&format!("{}{}\n", self.indent_str(), self.emit_stmt(stmt)));
        }
        if let Some(tail) = &f.body.tail {
            out.push_str(&format!("{}{}\n", self.indent_str(), self.emit_expr(tail)));
        }
        self.indent -= 1;
        out.push_str(&format!("{}}}\n", ind));
        out
    }

    fn emit_stmt(&self, stmt: &Stmt) -> String {
        match stmt {
            Stmt::Let { name, mutable, ty, init } => {
                let mut_str = if *mutable { "mut " } else { "" };
                let ty_str = ty.as_ref().map(|t| format!(": {}", self.emit_ty(t))).unwrap_or_default();
                let init_str = init.as_ref().map(|e| format!(" = {}", self.emit_expr_level0(e))).unwrap_or_default();
                format!("let {}{}{}{};", mut_str, name, ty_str, init_str)
            }
            Stmt::LetPat { pat, ty, init, .. } => {
                let ty_str = ty.as_ref().map(|t| format!(": {}", self.emit_ty(t))).unwrap_or_default();
                let init_str = init.as_ref().map(|e| format!(" = {}", self.emit_expr_level0(e))).unwrap_or_default();
                format!("let {}{}{};", emit_pat(pat), ty_str, init_str)
            }
            Stmt::Semi(e) => format!("{};", self.emit_expr(e)),
            Stmt::Expr(e) => self.emit_expr(e),
            Stmt::Item(item) => {
                let mut cg = Codegen { indent: self.indent, level: self.level, llm_mode: self.llm_mode };
                cg.emit_item(item)
            }
        }
    }

    fn emit_ty(&self, ty: &Ty) -> String {
        match ty {
            Ty::Named(s) => s.clone(),
            Ty::Unit => "()".to_string(),
            Ty::Never => "!".to_string(),
            Ty::Ref(mutable, inner) => {
                if *mutable { format!("&mut {}", self.emit_ty(inner)) }
                else { format!("&{}", self.emit_ty(inner)) }
            }
            Ty::Ptr(mutable, inner) => {
                if *mutable { format!("*mut {}", self.emit_ty(inner)) }
                else { format!("*const {}", self.emit_ty(inner)) }
            }
            Ty::Slice(inner) => format!("[{}]", self.emit_ty(inner)),
            Ty::Tuple(tys) => {
                if tys.is_empty() { "()".to_string() }
                else { format!("({})", tys.iter().map(|t| self.emit_ty(t)).collect::<Vec<_>>().join(", ")) }
            }
            Ty::Generic(name, args) => {
                format!("{}<{}>", name, args.iter().map(|a| self.emit_ty(a)).collect::<Vec<_>>().join(", "))
            }
            Ty::Lifetime(lt) => format!("'{}", lt),
        }
    }

    /// Emit expression with Level 0 clone() insertions where a move would occur.
    /// In `--llm-mode`, annotate each clone with an ownership comment for auditability.
    fn emit_expr_level0(&self, expr: &Expr) -> String {
        match expr {
            // Variable references in non-trivial positions get .clone() at Level 0
            // to prevent move errors
            Expr::Ident(name) => {
                if self.llm_mode {
                    format!("{}.clone() /* ownership: clone prevents move of `{}` */", name, name)
                } else {
                    format!("{}.clone()", name)
                }
            }
            _ => self.emit_expr(expr),
        }
    }

    fn emit_expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::Lit(lit) => emit_lit(lit),

            Expr::Ident(name) => name.clone(),

            Expr::Path(parts) => parts.join("::"),

            Expr::Unary(op, inner) => {
                let op_str = match op { UnOp::Neg => "-", UnOp::Not => "!" };
                format!("{}({})", op_str, self.emit_expr(inner))
            }

            Expr::Binary(op, lhs, rhs) => {
                // Level 4 (Prove): use checked arithmetic for +, -, * to surface overflows
                // as explicit panics rather than silent wrapping/UB.
                if self.level >= StrictnessLevel::Prove {
                    let checked = match op {
                        BinOp::Add => Some("checked_add"),
                        BinOp::Sub => Some("checked_sub"),
                        BinOp::Mul => Some("checked_mul"),
                        _ => None,
                    };
                    if let Some(method) = checked {
                        return format!(
                            "{}.{}({}).expect(\"arithmetic overflow\")",
                            self.emit_expr(lhs), method, self.emit_expr(rhs)
                        );
                    }
                }
                let op_str = match op {
                    BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*",
                    BinOp::Div => "/", BinOp::Rem => "%",
                    BinOp::Eq => "==", BinOp::Ne => "!=",
                    BinOp::Lt => "<", BinOp::Le => "<=", BinOp::Gt => ">", BinOp::Ge => ">=",
                    BinOp::And => "&&", BinOp::Or => "||",
                    BinOp::BitAnd => "&", BinOp::BitOr => "|", BinOp::BitXor => "^",
                    BinOp::Shl => "<<", BinOp::Shr => ">>",
                };
                format!("({} {} {})", self.emit_expr(lhs), op_str, self.emit_expr(rhs))
            }

            Expr::Assign(lhs, rhs) => format!("{} = {}", self.emit_expr(lhs), self.emit_expr(rhs)),

            Expr::OpAssign(op, lhs, rhs) => {
                let op_str = match op {
                    BinOp::Add => "+=", BinOp::Sub => "-=", BinOp::Mul => "*=",
                    BinOp::Div => "/=", BinOp::Rem => "%=", _ => "+=",
                };
                format!("{} {} {}", self.emit_expr(lhs), op_str, self.emit_expr(rhs))
            }

            Expr::Call { func, args } => {
                let args_str = args.iter().map(|a| self.emit_expr_level0(a)).collect::<Vec<_>>().join(", ");
                format!("{}({})", self.emit_expr(func), args_str)
            }

            Expr::MethodCall { receiver, method, args, .. } => {
                let args_str = args.iter().map(|a| self.emit_expr_level0(a)).collect::<Vec<_>>().join(", ");
                format!("{}.{}({})", self.emit_expr(receiver), method, args_str)
            }

            Expr::Field(base, field) => format!("{}.{}", self.emit_expr(base), field),

            Expr::Index(base, idx) => format!("{}[{}]", self.emit_expr(base), self.emit_expr(idx)),

            Expr::If { cond, then_block, else_block } => {
                let mut out = format!("if {} {{\n", self.emit_expr(cond));
                out.push_str(&self.emit_block_body(then_block));
                out.push('}');
                if let Some(else_expr) = else_block {
                    out.push_str(&format!(" else {}", self.emit_expr(else_expr)));
                }
                out
            }

            Expr::Block(block) => {
                let mut out = "{\n".to_string();
                out.push_str(&self.emit_block_body(block));
                out.push('}');
                out
            }

            Expr::Unsafe(block) => {
                let mut out = "unsafe {\n".to_string();
                out.push_str(&self.emit_block_body(block));
                out.push('}');
                out
            }

            Expr::Return(val) => {
                if let Some(v) = val { format!("return {}", self.emit_expr(v)) }
                else { "return".to_string() }
            }

            Expr::Break(label, val) => {
                let mut s = "break".to_string();
                if let Some(l) = label { s.push_str(&format!(" '{}", l)); }
                if let Some(v) = val { s.push_str(&format!(" {}", self.emit_expr(v))); }
                s
            }

            Expr::Continue(label) => {
                if let Some(l) = label { format!("continue '{}", l) }
                else { "continue".to_string() }
            }

            Expr::Macro { name, args } => {
                // Restore macro call syntax
                match name.as_str() {
                    "__for__" => {
                        // args: [pat_marker, iterable, body]
                        if args.len() >= 3 {
                            let var = match &args[0] {
                                Expr::Block(b) => {
                                    if let Some(Stmt::Expr(Expr::Ident(s))) = b.stmts.first() {
                                        s.trim_start_matches("__pat__").to_string()
                                    } else { "_".to_string() }
                                }
                                _ => "_".to_string(),
                            };
                            let iter = self.emit_expr_level0(&args[1]);
                            let body = self.emit_expr(&args[2]);
                            format!("for {} in {} {}", var, iter, body)
                        } else { "/* for */".to_string() }
                    }
                    _ => {
                        let args_str = args.iter().map(|a| self.emit_expr(a)).collect::<Vec<_>>().join(", ");
                        format!("{}!({})", name, args_str)
                    }
                }
            }

            Expr::Match { scrutinee, arms } => {
                // Loop sentinel
                if let [arm] = arms.as_slice() {
                    if matches!(&arm.pat, Pat::Ident(s) if s == "__loop__") {
                        return format!("loop {}", self.emit_expr(&arm.body));
                    }
                }
                let mut out = format!("match {} {{\n", self.emit_expr(scrutinee));
                for arm in arms {
                    let guard = arm.guard.as_ref().map(|g| format!(" if {}", self.emit_expr(g))).unwrap_or_default();
                    out.push_str(&format!("    {}{} => {},\n",
                        emit_pat(&arm.pat), guard, self.emit_expr(&arm.body)));
                }
                out.push('}');
                out
            }

            Expr::Closure { params, body } => {
                use crate::ast::ClosureParam;
                let ps: Vec<String> = params.iter().map(|p| match p {
                    ClosureParam::Simple(n) => n.clone(),
                    ClosureParam::Tuple(ns) => format!("({})", ns.join(", ")),
                    ClosureParam::Pat(_) => "_".into(),
                }).collect();
                format!("|{}| {}", ps.join(", "), self.emit_expr(body))
            }

            Expr::StructLit { name, fields } => {
                let fields_str = fields.iter()
                    .map(|(n, v)| format!("{}: {}", n, self.emit_expr_level0(v)))
                    .collect::<Vec<_>>().join(", ");
                format!("{} {{ {} }}", name, fields_str)
            }

            Expr::Array(elems) => {
                format!("[{}]", elems.iter().map(|e| self.emit_expr(e)).collect::<Vec<_>>().join(", "))
            }

            Expr::Tuple(elems) => {
                format!("({})", elems.iter().map(|e| self.emit_expr(e)).collect::<Vec<_>>().join(", "))
            }

            Expr::Range { start, end, inclusive } => {
                let s = start.as_ref().map(|e| self.emit_expr(e)).unwrap_or_default();
                let e = end.as_ref().map(|e| self.emit_expr(e)).unwrap_or_default();
                if *inclusive { format!("{}..={}", s, e) } else { format!("{}..{}", s, e) }
            }

            Expr::Cast(inner, ty) => format!("({} as {})", self.emit_expr(inner), self.emit_ty(ty)),

            Expr::Ref { mutable, expr } => {
                if *mutable { format!("&mut {}", self.emit_expr(expr)) }
                else { format!("&{}", self.emit_expr(expr)) }
            }

            Expr::Deref(inner) => format!("*{}", self.emit_expr(inner)),
            Expr::Try(inner) => format!("{}?", self.emit_expr(inner)),

            // `.await` — emitted directly; at Level 4 the caller already has async context.
            Expr::Await(inner) => format!("{}.await", self.emit_expr(inner)),
        }
    }

    fn emit_block_body(&self, block: &Block) -> String {
        let mut out = String::new();
        for stmt in &block.stmts {
            out.push_str(&format!("    {}\n", self.emit_stmt(stmt)));
        }
        if let Some(tail) = &block.tail {
            out.push_str(&format!("    {}\n", self.emit_expr(tail)));
        }
        out
    }
}

fn emit_lit(lit: &Lit) -> String {
    match lit {
        Lit::Int(n)   => n.to_string(),
        Lit::Float(f) => {
            if f.fract() == 0.0 { format!("{:.1}", f) } else { f.to_string() }
        }
        Lit::Bool(b)  => b.to_string(),
        Lit::Str(s)   => format!("{:?}", s),
        Lit::Char(c)  => format!("{:?}", c),
    }
}

fn emit_pat(pat: &Pat) -> String {
    match pat {
        Pat::Wild => "_".to_string(),
        Pat::Ident(s) => s.clone(),
        Pat::Lit(l) => emit_lit(l),
        Pat::Tuple(ps) => format!("({})", ps.iter().map(emit_pat).collect::<Vec<_>>().join(", ")),
        Pat::Struct { name, fields, rest } => {
            let mut out = format!("{} {{ ", name);
            for (n, p) in fields { out.push_str(&format!("{}: {}, ", n, emit_pat(p))); }
            if *rest { out.push_str(".."); }
            out.push('}');
            out
        }
        Pat::TupleStruct { name, fields } => {
            format!("{}({})", name, fields.iter().map(emit_pat).collect::<Vec<_>>().join(", "))
        }
        Pat::Or(ps) => ps.iter().map(emit_pat).collect::<Vec<_>>().join(" | "),
        Pat::Range(lo, hi, inc) => {
            if *inc { format!("{}..={}", emit_lit(lo), emit_lit(hi)) }
            else { format!("{}..{}", emit_lit(lo), emit_lit(hi)) }
        }
        Pat::Ref(inner) => format!("&{}", emit_pat(inner)),
        Pat::Bind { name, pat } => format!("{} @ {}", name, emit_pat(pat)),
        Pat::Slice { before, rest, has_rest, after } => {
            let mut parts: Vec<String> = before.iter().map(emit_pat).collect();
            if *has_rest {
                let rest_str = if let Some(name) = rest {
                    format!("{} @ ..", name)
                } else {
                    "..".to_string()
                };
                parts.push(rest_str);
            }
            parts.extend(after.iter().map(emit_pat));
            format!("[{}]", parts.join(", "))
        }
    }
}
