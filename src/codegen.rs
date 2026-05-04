use crate::ast::*;
use crate::strictness::StrictnessLevel;

pub struct Codegen {
    indent: usize,
    /// The strictness level controls which safety features and annotations are emitted.
    pub level: StrictnessLevel,
    /// When true, ownership-transfer comments are injected at every clone/move site,
    /// making LLM-generated code auditable by human reviewers.
    pub llm_mode: bool,
    /// When true, emit `pub` on every item, struct field, and impl method.
    /// Set while emitting the body of `mod NAME { ... }` since the parser
    /// strips author-supplied `pub` (proper visibility tracking is in
    /// crust-1x4). Without this, items inside a module are private and
    /// outside callers hit E0603 / E0624.
    force_pub: bool,
}

impl Codegen {
    /// Kept for backward compatibility and direct test usage.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Codegen {
            indent: 0,
            level: StrictnessLevel::Explore,
            llm_mode: false,
            force_pub: false,
        }
    }

    pub fn with_level(level: StrictnessLevel) -> Self {
        Codegen {
            indent: 0,
            level,
            llm_mode: false,
            force_pub: false,
        }
    }

    /// `pub ` prefix to emit before each item / field / method when inside a
    /// `mod` block. Empty otherwise.
    fn pub_kw(&self) -> &'static str {
        if self.force_pub {
            "pub "
        } else {
            ""
        }
    }

    /// Format the generic-parameter list for an introducing position
    /// (struct/enum/fn/impl/trait header). At Level <Ship, attach a
    /// `: Clone` bound so Crust's implicit `.clone()` emission compiles
    /// against bare type parameters. At Level Ship+, leave bounds alone.
    fn format_introducing_generics(&self, names: &[String]) -> String {
        if self.level < StrictnessLevel::Ship {
            format_generics_with_clone_bound(names)
        } else {
            format_generics(names)
        }
    }

    pub fn emit_program(&mut self, items: &[Item]) -> String {
        let mut out = String::new();
        // Suppress unused-import / unused-variable warnings on the generated file.
        // Crust emits a fixed std prelude regardless of whether the user's program
        // touches each item, so these warnings are noise.
        out.push_str(
            "#![allow(unused_imports, unused_variables, unused_mut, unused_parens, dead_code)]\n",
        );
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
                format!(
                    "const {}: {} = {};\n",
                    name,
                    self.emit_ty(ty),
                    self.emit_expr(value)
                )
            }
            Item::TypeAlias { name, ty } => {
                format!("type {} = {};\n", name, self.emit_ty(ty))
            }
            Item::Trait {
                name,
                methods,
                generics,
            } => {
                let mut out = format!(
                    "trait {}{} {{\n",
                    name,
                    self.format_introducing_generics(generics)
                );
                for m in methods {
                    out.push_str(&format!("    {}", self.emit_fn(m)));
                }
                out.push_str("}\n");
                out
            }
            Item::Mod { name, items } => {
                let pub_kw = self.pub_kw();
                let mut out = format!("{}mod {} {{\n", pub_kw, name);
                self.indent += 1;
                let saved_force_pub = self.force_pub;
                // At Level <Ship, expose every item, field, and method inside
                // the module so outside callers can reach them. Crust does not
                // yet track per-item `pub` (crust-1x4). At Level Ship+, leave
                // visibility alone — at that strictness Crust is supposed to
                // be source-level rustc-equivalent.
                self.force_pub = self.level < StrictnessLevel::Ship;
                for item in items {
                    let body = self.emit_item(item);
                    for line in body.lines() {
                        if line.is_empty() {
                            out.push('\n');
                        } else {
                            out.push_str(&format!("{}{}\n", self.indent_str(), line));
                        }
                    }
                }
                self.force_pub = saved_force_pub;
                self.indent -= 1;
                out.push_str("}\n");
                out
            }
        }
    }

    fn emit_struct(&mut self, s: &StructDef) -> String {
        let mut out = String::new();
        out.push_str(&self.emit_type_attrs(&s.attrs));
        let kw = self.pub_kw();
        let field_pub = self.pub_kw();
        out.push_str(&format!(
            "{}struct {}{} {{\n",
            kw,
            s.name,
            self.format_introducing_generics(&s.generics)
        ));
        for (name, ty) in &s.fields {
            out.push_str(&format!(
                "    {}{}: {},\n",
                field_pub,
                name,
                self.emit_ty(ty)
            ));
        }
        out.push_str("}\n");
        out
    }

    fn emit_enum(&mut self, e: &EnumDef) -> String {
        let mut out = String::new();
        out.push_str(&self.emit_type_attrs(&e.attrs));
        let kw = self.pub_kw();
        out.push_str(&format!(
            "{}enum {}{} {{\n",
            kw,
            e.name,
            self.format_introducing_generics(&e.generics)
        ));
        for v in &e.variants {
            out.push_str("    ");
            out.push_str(&v.name);
            match &v.data {
                VariantData::Unit => out.push_str(",\n"),
                VariantData::Tuple(tys) => {
                    out.push('(');
                    out.push_str(
                        &tys.iter()
                            .map(|t| self.emit_ty(t))
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
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
        let impl_generics = self.format_introducing_generics(&i.generics);
        let type_args = format_generics(&i.type_args);
        if let Some(tr) = &i.trait_name {
            out.push_str(&format!(
                "impl{} {} for {}{} {{\n",
                impl_generics, tr, i.type_name, type_args
            ));
        } else {
            out.push_str(&format!(
                "impl{} {}{} {{\n",
                impl_generics, i.type_name, type_args
            ));
        }
        self.indent += 1;
        for (name, ty, expr) in &i.consts {
            out.push_str(&format!(
                "{}const {}: {} = {};\n",
                "    ".repeat(self.indent),
                name,
                self.emit_ty(ty),
                self.emit_expr(expr)
            ));
        }
        for m in &i.methods {
            out.push_str(&self.emit_fn(m));
        }
        self.indent -= 1;
        out.push_str("}\n");
        out
    }

    /// Emit `#[derive(...)]` (and any other unknown attrs) for a struct or enum.
    /// At Level <Ship Crust auto-derives `Clone, Debug, PartialEq`; merge any
    /// author-supplied derives in to avoid duplicates and preserve intent.
    /// Author-supplied non-derive attrs (e.g. `#[repr(C)]`) are passed through verbatim.
    fn emit_type_attrs(&self, attrs: &[Attr]) -> String {
        let mut out = String::new();
        let mut author_derives: Vec<String> = Vec::new();
        for attr in attrs {
            if let Attr::Unknown(content) = attr {
                if let Some(rest) = content.strip_prefix("derive(") {
                    if let Some(inner) = rest.strip_suffix(')') {
                        for d in inner.split(',') {
                            let d = d.trim().to_string();
                            if !d.is_empty() && !author_derives.contains(&d) {
                                author_derives.push(d);
                            }
                        }
                        continue;
                    }
                }
                out.push_str(&format!("#[{}]\n", content));
            }
        }
        let mut all_derives: Vec<String> = if self.level < StrictnessLevel::Ship {
            vec![
                "Clone".to_string(),
                "Debug".to_string(),
                "PartialEq".to_string(),
            ]
        } else {
            Vec::new()
        };
        for d in author_derives {
            if !all_derives.contains(&d) {
                all_derives.push(d);
            }
        }
        if !all_derives.is_empty() {
            out.push_str(&format!("#[derive({})]\n", all_derives.join(", ")));
        }
        out
    }

    fn emit_fn(&mut self, f: &FnDef) -> String {
        let ind = self.indent_str();
        let pub_kw = self.pub_kw();

        // Emit attributes.  At Level 3+, re-emit all unknown attrs (e.g. derive, allow).
        // Crust-specific attrs (requires/ensures/invariant/pure) are emitted as comments
        // so the generated Rust is still valid.
        let mut out = String::new();
        for attr in &f.attrs {
            match attr {
                Attr::Pure => out.push_str(&format!(
                    "{}// #[pure] — verified side-effect-free by Crust\n",
                    ind
                )),
                Attr::Requires(expr) => out.push_str(&format!(
                    "{}// #[requires({})]\n",
                    ind,
                    self.emit_expr(expr)
                )),
                Attr::Ensures(expr) => {
                    out.push_str(&format!("{}// #[ensures({})]\n", ind, self.emit_expr(expr)))
                }
                Attr::Invariant(expr) => out.push_str(&format!(
                    "{}// #[invariant({})]\n",
                    ind,
                    self.emit_expr(expr)
                )),
                Attr::Unknown(s) if self.level >= StrictnessLevel::Ship => {
                    out.push_str(&format!("{}#[{}]\n", ind, s))
                }
                Attr::Unknown(_) => {} // lower levels: skip non-crust attrs (already derived)
            }
        }

        let params = f
            .params
            .iter()
            .map(|p| {
                if p.is_self {
                    // Distinguish &self / &mut self / self / mut self based on the
                    // captured `Ty::Ref(mutable, _)` and `mutable` fields.
                    match (&p.ty, p.mutable) {
                        (Ty::Ref(true, _), _) => "&mut self".to_string(),
                        (Ty::Ref(false, _), _) => "&self".to_string(),
                        (_, true) => "mut self".to_string(),
                        _ => "self".to_string(),
                    }
                } else if p.mutable {
                    format!("mut {}: {}", p.name, self.emit_ty(&p.ty))
                } else {
                    format!("{}: {}", p.name, self.emit_ty(&p.ty))
                }
            })
            .collect::<Vec<_>>()
            .join(", ");

        // Lifetime-elision rescue at Level <Ship: if the return type is a
        // bare `&T` / `&mut T` (no explicit lifetime) and the function has
        // no input references whose lifetime rustc could elide to, promote
        // the return to `&'static T`. This handles `fn name(d: Direction)
        // -> &str { match … }` where the body returns string literals
        // (genuinely 'static) — fixes E0106 without needing real
        // lifetime inference (crust-1x4).
        let ret_ty_promoted = f.ret_ty.as_ref().map(|ty| {
            if self.level < StrictnessLevel::Ship && needs_static_promotion(ty, &f.params) {
                promote_to_static(ty)
            } else {
                ty.clone()
            }
        });
        let ret = if let Some(ty) = &ret_ty_promoted {
            format!(" -> {}", self.emit_ty(ty))
        } else {
            String::new()
        };

        let async_kw = if f.is_async { "async " } else { "" };
        let fn_generics = self.format_introducing_generics(&f.generics);
        out.push_str(&format!(
            "{}{}{}fn {}{}({}){} {{\n",
            ind, pub_kw, async_kw, f.name, fn_generics, params, ret
        ));
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
            Stmt::Let {
                name,
                mutable,
                ty,
                init,
            } => {
                let mut_str = if *mutable { "mut " } else { "" };
                let ty_str = ty
                    .as_ref()
                    .map(|t| format!(": {}", self.emit_ty(t)))
                    .unwrap_or_default();
                let init_str = init
                    .as_ref()
                    .map(|e| format!(" = {}", self.emit_expr_level0(e)))
                    .unwrap_or_default();
                format!("let {}{}{}{};", mut_str, name, ty_str, init_str)
            }
            Stmt::LetPat {
                pat,
                ty,
                init,
                else_block,
            } => {
                let ty_str = ty
                    .as_ref()
                    .map(|t| format!(": {}", self.emit_ty(t)))
                    .unwrap_or_default();
                let init_str = init
                    .as_ref()
                    .map(|e| format!(" = {}", self.emit_expr_level0(e)))
                    .unwrap_or_default();
                // `let PAT = EXPR else { … };` (let-else) — refutable
                // pattern with explicit divergence. Without the else, rustc
                // rejects refutable patterns in let bindings (E0005). At
                // Level <Ship, when neither the user nor the parser supplied
                // an else block but the pattern is refutable (Slice with
                // varying length, TupleStruct, Struct, Or, Range, Bind),
                // synthesise an `else { unreachable!() }` so the program
                // compiles under Crust's "Level 0 forgives" philosophy.
                let else_str = match else_block {
                    Some(b) => format!(" else {}", self.emit_block_as_expr(b)),
                    None if pat_is_refutable(pat) && self.level < StrictnessLevel::Ship => {
                        " else { unreachable!() }".to_string()
                    }
                    None => String::new(),
                };
                format!("let {}{}{}{};", emit_pat(pat), ty_str, init_str, else_str)
            }
            Stmt::Semi(e) => format!("{};", self.emit_expr(e)),
            Stmt::Expr(e) => self.emit_expr(e),
            Stmt::Item(item) => {
                let mut cg = Codegen {
                    indent: self.indent,
                    level: self.level,
                    llm_mode: self.llm_mode,
                    force_pub: self.force_pub,
                };
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
                if *mutable {
                    format!("&mut {}", self.emit_ty(inner))
                } else {
                    format!("&{}", self.emit_ty(inner))
                }
            }
            Ty::RefLt(mutable, lt, inner) => {
                if *mutable {
                    format!("&'{} mut {}", lt, self.emit_ty(inner))
                } else {
                    format!("&'{} {}", lt, self.emit_ty(inner))
                }
            }
            Ty::Ptr(mutable, inner) => {
                if *mutable {
                    format!("*mut {}", self.emit_ty(inner))
                } else {
                    format!("*const {}", self.emit_ty(inner))
                }
            }
            Ty::Slice(inner) => format!("[{}]", self.emit_ty(inner)),
            Ty::Tuple(tys) => {
                if tys.is_empty() {
                    "()".to_string()
                } else {
                    format!(
                        "({})",
                        tys.iter()
                            .map(|t| self.emit_ty(t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            Ty::Generic(name, args) => {
                format!(
                    "{}<{}>",
                    name,
                    args.iter()
                        .map(|a| self.emit_ty(a))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Ty::Lifetime(lt) => format!("'{}", lt),
            Ty::FnPtr { kind, params, ret } => {
                let params_str = params
                    .iter()
                    .map(|t| self.emit_ty(t))
                    .collect::<Vec<_>>()
                    .join(", ");
                let prefix = if kind.is_empty() { "fn" } else { kind.as_str() };
                let ret_str = if matches!(ret.as_ref(), Ty::Unit) {
                    String::new()
                } else {
                    format!(" -> {}", self.emit_ty(ret))
                };
                format!("{}({}){}", prefix, params_str, ret_str)
            }
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
                    format!(
                        "{}.clone() /* ownership: clone prevents move of `{}` */",
                        name, name
                    )
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
                let op_str = match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "!",
                };
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
                            self.emit_expr(lhs),
                            method,
                            self.emit_expr(rhs)
                        );
                    }
                }
                let op_str = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Rem => "%",
                    BinOp::Eq => "==",
                    BinOp::Ne => "!=",
                    BinOp::Lt => "<",
                    BinOp::Le => "<=",
                    BinOp::Gt => ">",
                    BinOp::Ge => ">=",
                    BinOp::And => "&&",
                    BinOp::Or => "||",
                    BinOp::BitAnd => "&",
                    BinOp::BitOr => "|",
                    BinOp::BitXor => "^",
                    BinOp::Shl => "<<",
                    BinOp::Shr => ">>",
                };
                format!(
                    "({} {} {})",
                    self.emit_expr(lhs),
                    op_str,
                    self.emit_expr(rhs)
                )
            }

            Expr::Assign(lhs, rhs) => format!("{} = {}", self.emit_expr(lhs), self.emit_expr(rhs)),

            Expr::OpAssign(op, lhs, rhs) => {
                let op_str = match op {
                    BinOp::Add => "+=",
                    BinOp::Sub => "-=",
                    BinOp::Mul => "*=",
                    BinOp::Div => "/=",
                    BinOp::Rem => "%=",
                    _ => "+=",
                };
                format!("{} {} {}", self.emit_expr(lhs), op_str, self.emit_expr(rhs))
            }

            Expr::Call { func, args } => {
                let args_str = args
                    .iter()
                    .map(|a| self.emit_expr_level0(a))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", self.emit_expr(func), args_str)
            }

            Expr::MethodCall {
                receiver,
                method,
                turbofish,
                args,
            } => {
                // Ownership-relaxation rewrite for closure args inside an
                // `.iter()` chain at Level <Ship (crust-ovw).
                //
                // After `.iter().cloned()`, owned-by-value iterator methods
                // (`map`, `fold`, `for_each`, …) receive `T`, so user-written
                // `*p` derefs of the closure param become invalid (E0614).
                // Reference-taking iterator methods (`filter`, `any`, `all`,
                // `find`, `position`, `take_while`, `skip_while`) still
                // receive `&T`, so the user's `*p` stays correct.
                //
                // Strip `*p` only in the owned-by-value branch.
                let strip_in_closures = self.level < StrictnessLevel::Ship
                    && chain_starts_with_iter(receiver)
                    && iter_method_takes_owned(method);

                let args_str = args
                    .iter()
                    .map(|a| {
                        if strip_in_closures {
                            if let Expr::Closure { params, body } = a {
                                let names = closure_param_names(params);
                                let mut new_body = (**body).clone();
                                strip_param_derefs(&mut new_body, &names);
                                let rewritten = Expr::Closure {
                                    params: params.clone(),
                                    body: Box::new(new_body),
                                };
                                return self.emit_expr_level0(&rewritten);
                            }
                        }
                        self.emit_expr_level0(a)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");

                let tf = match turbofish {
                    Some(tys) if !tys.is_empty() => {
                        let parts = tys
                            .iter()
                            .map(|t| self.emit_ty(t))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("::<{}>", parts)
                    }
                    _ => String::new(),
                };
                // At Level 0–2, Crust treats `.iter()` as yielding owned values
                // (the interp clones; the Rust output should match). Inject
                // `.cloned()` for the common `xs.iter()` case so closures and
                // collect chains see `T` rather than `&T`. At Level 3+ the
                // developer is expected to deal with references explicitly.
                let needs_cloned = self.level < StrictnessLevel::Ship
                    && method == "iter"
                    && args.is_empty()
                    && turbofish.is_none();
                let base = format!(
                    "{}.{}{}({})",
                    self.emit_expr(receiver),
                    method,
                    tf,
                    args_str
                );
                if needs_cloned {
                    format!("{}.cloned()", base)
                } else {
                    base
                }
            }

            Expr::Field(base, field) => format!("{}.{}", self.emit_expr(base), field),

            Expr::Index(base, idx) => format!("{}[{}]", self.emit_expr(base), self.emit_expr(idx)),

            Expr::If {
                cond,
                then_block,
                else_block,
            } => {
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
                if let Some(v) = val {
                    format!("return {}", self.emit_expr(v))
                } else {
                    "return".to_string()
                }
            }

            Expr::Break(label, val) => {
                let mut s = "break".to_string();
                if let Some(l) = label {
                    s.push_str(&format!(" '{}", l));
                }
                if let Some(v) = val {
                    s.push_str(&format!(" {}", self.emit_expr(v)));
                }
                s
            }

            Expr::Continue(label) => {
                if let Some(l) = label {
                    format!("continue '{}", l)
                } else {
                    "continue".to_string()
                }
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
                                    } else {
                                        "_".to_string()
                                    }
                                }
                                _ => "_".to_string(),
                            };
                            let iter = self.emit_expr_level0(&args[1]);
                            let body = self.emit_expr(&args[2]);
                            format!("for {} in {} {}", var, iter, body)
                        } else {
                            "/* for */".to_string()
                        }
                    }
                    _ => {
                        let args_str = args
                            .iter()
                            .map(|a| self.emit_expr(a))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{}!({})", name, args_str)
                    }
                }
            }

            Expr::Match { scrutinee, arms } => {
                // Loop sentinel — `loop` body must be a brace-block in Rust,
                // so wrap any non-Block body. Without the wrap, a desugared
                // `while let` produces `loop match …` which rustc rejects.
                if let [arm] = arms.as_slice() {
                    if matches!(&arm.pat, Pat::Ident(s) if s == "__loop__") {
                        let body = self.emit_expr(&arm.body);
                        if matches!(arm.body, Expr::Block(_)) {
                            return format!("loop {}", body);
                        } else {
                            return format!("loop {{ {} }}", body);
                        }
                    }
                }
                let mut out = format!("match {} {{\n", self.emit_expr(scrutinee));
                for arm in arms {
                    let guard = arm
                        .guard
                        .as_ref()
                        .map(|g| format!(" if {}", self.emit_expr(g)))
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "    {}{} => {},\n",
                        emit_pat(&arm.pat),
                        guard,
                        self.emit_expr(&arm.body)
                    ));
                }
                out.push('}');
                out
            }

            Expr::Closure { params, body } => {
                use crate::ast::ClosureParam;
                let ps: Vec<String> = params
                    .iter()
                    .map(|p| match p {
                        ClosureParam::Simple(n) => n.clone(),
                        ClosureParam::Tuple(ns) => format!("({})", ns.join(", ")),
                        ClosureParam::Pat(_) => "_".into(),
                    })
                    .collect();
                format!("|{}| {}", ps.join(", "), self.emit_expr(body))
            }

            Expr::StructLit { name, fields } => {
                let fields_str = fields
                    .iter()
                    .map(|(n, v)| format!("{}: {}", n, self.emit_expr_level0(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} {{ {} }}", name, fields_str)
            }

            Expr::Array(elems) => {
                format!(
                    "[{}]",
                    elems
                        .iter()
                        .map(|e| self.emit_expr(e))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }

            Expr::Tuple(elems) => {
                format!(
                    "({})",
                    elems
                        .iter()
                        .map(|e| self.emit_expr(e))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }

            Expr::Range {
                start,
                end,
                inclusive,
            } => {
                let s = start
                    .as_ref()
                    .map(|e| self.emit_expr(e))
                    .unwrap_or_default();
                let e = end.as_ref().map(|e| self.emit_expr(e)).unwrap_or_default();
                // Always wrap Range in parens so it composes in
                // method-call receiver position: `1..=100.sum()` would
                // otherwise parse as `1..=(100.sum())` since `.` binds
                // tighter than `..=`.
                if *inclusive {
                    format!("({}..={})", s, e)
                } else {
                    format!("({}..{})", s, e)
                }
            }

            Expr::Cast(inner, ty) => format!("({} as {})", self.emit_expr(inner), self.emit_ty(ty)),

            Expr::Ref { mutable, expr } => {
                if *mutable {
                    format!("&mut {}", self.emit_expr(expr))
                } else {
                    format!("&{}", self.emit_expr(expr))
                }
            }

            Expr::Deref(inner) => format!("*{}", self.emit_expr(inner)),
            Expr::Try(inner) => format!("{}?", self.emit_expr(inner)),

            // `.await` — emitted directly; at Level 4 the caller already has async context.
            Expr::Await(inner) => format!("{}.await", self.emit_expr(inner)),
        }
    }

    fn emit_block_as_expr(&self, block: &Block) -> String {
        let mut out = String::from("{\n");
        out.push_str(&self.emit_block_body(block));
        out.push('}');
        out
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

/// True if the pattern can fail to match a value of the bound expression's
/// type. `Ident` and `Wild` are irrefutable; everything else is generally
/// refutable. Crust uses this to decide when a `let PAT = EXPR;` binding
/// needs an auto-injected `else { unreachable!() }` for rustc's E0005.
fn pat_is_refutable(pat: &Pat) -> bool {
    match pat {
        Pat::Wild | Pat::Ident(_) => false,
        Pat::Tuple(ps) => ps.iter().any(pat_is_refutable),
        // All other patterns are refutable in general.
        _ => true,
    }
}

/// Iterator methods whose closures receive *owned* `Self::Item` (per the
/// std::iter::Iterator signatures). After Crust's `.iter().cloned()`
/// injection these closures see `T`, so user `*p` derefs are invalid and
/// need stripping. Reference-taking methods receive `&T` and keep `*p`
/// as written.
fn iter_method_takes_owned(name: &str) -> bool {
    matches!(
        name,
        // FnMut(Self::Item) -> ...
        "map"
            | "filter_map"
            | "flat_map"
            | "for_each"
            | "scan"
            | "any"
            | "all"
            | "position"
            // FnMut(B, Self::Item) -> B  (the Self::Item arg is owned)
            | "fold"
            | "reduce"
    )
}

/// True if `expr` is a method-call chain whose root receiver is `.iter()`.
/// Used by the ownership-relaxation rewrite (crust-ovw) to decide whether
/// closure-body `*p` strip-down is in scope.
fn chain_starts_with_iter(expr: &Expr) -> bool {
    let mut cur = expr;
    loop {
        match cur {
            Expr::MethodCall {
                receiver,
                method,
                args,
                turbofish,
            } => {
                if method == "iter" && args.is_empty() && turbofish.is_none() {
                    return true;
                }
                cur = receiver;
            }
            _ => return false,
        }
    }
}

/// Collect the simple parameter names of a closure (`|x, y| …` → `["x", "y"]`).
/// Tuple- and pattern-shaped closure params don't bind a single identifier so
/// they're not eligible for the deref-strip rewrite.
fn closure_param_names(params: &[ClosureParam]) -> Vec<String> {
    params
        .iter()
        .filter_map(|p| match p {
            ClosureParam::Simple(n) => Some(n.clone()),
            _ => None,
        })
        .collect()
}

/// Walk `expr` and replace `Expr::Deref(Expr::Ident(p))` with `Expr::Ident(p)`
/// whenever `p` is in `params`. Used to neutralise user-written `*p` derefs
/// in closure bodies after Crust has already lowered the iterator chain to
/// yield owned `T` (via `.iter().cloned()`).
fn strip_param_derefs(expr: &mut Expr, params: &[String]) {
    // Fixpoint over the current node so `**x` → `x`.
    loop {
        if let Expr::Deref(inner) = expr {
            if let Expr::Ident(name) = inner.as_ref() {
                if params.contains(name) {
                    *expr = Expr::Ident(name.clone());
                    continue;
                }
            }
        }
        break;
    }
    match expr {
        Expr::Unary(_, e)
        | Expr::Deref(e)
        | Expr::Try(e)
        | Expr::Await(e)
        | Expr::Cast(e, _)
        | Expr::Field(e, _)
        | Expr::Ref { expr: e, .. } => strip_param_derefs(e, params),
        Expr::Binary(_, l, r)
        | Expr::Assign(l, r)
        | Expr::OpAssign(_, l, r)
        | Expr::Index(l, r) => {
            strip_param_derefs(l, params);
            strip_param_derefs(r, params);
        }
        Expr::Call { func, args } => {
            strip_param_derefs(func, params);
            for a in args {
                strip_param_derefs(a, params);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            strip_param_derefs(receiver, params);
            for a in args {
                strip_param_derefs(a, params);
            }
        }
        Expr::If {
            cond,
            then_block,
            else_block,
        } => {
            strip_param_derefs(cond, params);
            strip_block_param_derefs(then_block, params);
            if let Some(eb) = else_block {
                strip_param_derefs(eb, params);
            }
        }
        Expr::Match { scrutinee, arms } => {
            strip_param_derefs(scrutinee, params);
            for arm in arms {
                if let Some(g) = &mut arm.guard {
                    strip_param_derefs(g, params);
                }
                strip_param_derefs(&mut arm.body, params);
            }
        }
        Expr::Block(b) | Expr::Unsafe(b) => strip_block_param_derefs(b, params),
        Expr::Closure { params: cp, body } => {
            // A nested closure introduces its own params; don't strip those,
            // but our params still apply if they're referenced in the body.
            // Build the *difference* set by removing names the inner closure
            // shadows. (Examples don't shadow; correctness over pragmatism.)
            let inner: Vec<String> = closure_param_names(cp);
            let outer_only: Vec<String> = params
                .iter()
                .filter(|p| !inner.contains(p))
                .cloned()
                .collect();
            strip_param_derefs(body, &outer_only);
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                strip_param_derefs(e, params);
            }
        }
        Expr::Array(elems) | Expr::Tuple(elems) | Expr::Macro { args: elems, .. } => {
            for e in elems {
                strip_param_derefs(e, params);
            }
        }
        Expr::Range { start, end, .. } => {
            if let Some(s) = start {
                strip_param_derefs(s, params);
            }
            if let Some(e) = end {
                strip_param_derefs(e, params);
            }
        }
        Expr::Return(Some(e)) | Expr::Break(_, Some(e)) => strip_param_derefs(e, params),
        // Leaves
        Expr::Lit(_)
        | Expr::Ident(_)
        | Expr::Path(_)
        | Expr::Continue(_)
        | Expr::Return(None)
        | Expr::Break(_, None) => {}
    }
}

fn strip_block_param_derefs(block: &mut Block, params: &[String]) {
    for stmt in &mut block.stmts {
        match stmt {
            Stmt::Semi(e) | Stmt::Expr(e) => strip_param_derefs(e, params),
            Stmt::Let { init: Some(e), .. } => strip_param_derefs(e, params),
            Stmt::LetPat { init: Some(e), .. } => strip_param_derefs(e, params),
            _ => {}
        }
    }
    if let Some(tail) = &mut block.tail {
        strip_param_derefs(tail, params);
    }
}

/// True if a function with this return type and these params needs the
/// `&'static` promotion at codegen time. Triggers when the return is a bare
/// `&T` (no lifetime) and there are zero `&` parameters for elision to bind
/// to.
fn needs_static_promotion(ret: &Ty, params: &[Param]) -> bool {
    let bare_ref = matches!(ret, Ty::Ref(_, _));
    if !bare_ref {
        return false;
    }
    let any_ref_param = params.iter().any(|p| {
        matches!(p.ty, Ty::Ref(_, _) | Ty::RefLt(_, _, _))
            || (p.is_self && matches!(p.ty, Ty::Ref(_, _) | Ty::RefLt(_, _, _)))
    });
    !any_ref_param
}

/// Convert `Ty::Ref(mut, T)` → `Ty::RefLt(mut, "static", T)`. Used by the
/// lifetime-elision rescue above.
fn promote_to_static(ty: &Ty) -> Ty {
    match ty {
        Ty::Ref(mutable, inner) => Ty::RefLt(*mutable, "static".to_string(), inner.clone()),
        other => other.clone(),
    }
}

/// Format a generic-parameter list for codegen: `[]` → `""`,
/// `["T"]` → `"<T>"`, `["T", "U"]` → `"<T, U>"`.
fn format_generics(names: &[String]) -> String {
    if names.is_empty() {
        String::new()
    } else {
        format!("<{}>", names.join(", "))
    }
}

/// Same as `format_generics` but with a `: Clone` bound on each parameter.
/// Used at Level <Ship for type-introducing positions (struct, enum, fn,
/// impl, trait) so Crust's implicit `.clone()` emission on identifiers
/// doesn't fail with "method `clone` not found" on bare-generic values.
/// Type-application positions (e.g. `Queue<T>`) keep the bare-name form.
fn format_generics_with_clone_bound(names: &[String]) -> String {
    if names.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = names.iter().map(|n| format!("{}: Clone", n)).collect();
        format!("<{}>", parts.join(", "))
    }
}

fn emit_lit(lit: &Lit) -> String {
    match lit {
        Lit::Int(n) => n.to_string(),
        Lit::Float(f) => {
            if f.fract() == 0.0 {
                format!("{:.1}", f)
            } else {
                f.to_string()
            }
        }
        Lit::Bool(b) => b.to_string(),
        Lit::Str(s) => format!("{:?}", s),
        Lit::Char(c) => format!("{:?}", c),
    }
}

fn emit_pat(pat: &Pat) -> String {
    match pat {
        Pat::Wild => "_".to_string(),
        Pat::Ident(s) => s.clone(),
        Pat::Lit(l) => emit_lit(l),
        Pat::Tuple(ps) => format!(
            "({})",
            ps.iter().map(emit_pat).collect::<Vec<_>>().join(", ")
        ),
        Pat::Struct { name, fields, rest } => {
            let mut out = format!("{} {{ ", name);
            for (n, p) in fields {
                out.push_str(&format!("{}: {}, ", n, emit_pat(p)));
            }
            if *rest {
                out.push_str("..");
            }
            out.push('}');
            out
        }
        Pat::TupleStruct { name, fields } => {
            format!(
                "{}({})",
                name,
                fields.iter().map(emit_pat).collect::<Vec<_>>().join(", ")
            )
        }
        Pat::Or(ps) => ps.iter().map(emit_pat).collect::<Vec<_>>().join(" | "),
        Pat::Range(lo, hi, inc) => {
            if *inc {
                format!("{}..={}", emit_lit(lo), emit_lit(hi))
            } else {
                format!("{}..{}", emit_lit(lo), emit_lit(hi))
            }
        }
        Pat::Ref(inner) => format!("&{}", emit_pat(inner)),
        Pat::Bind { name, pat } => format!("{} @ {}", name, emit_pat(pat)),
        Pat::Slice {
            before,
            rest,
            has_rest,
            after,
        } => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn parse_and_emit(src: &str) -> String {
        let tokens = Lexer::new(src).tokenize().expect("tokenize");
        let prog = Parser::new(tokens).parse_program().expect("parse");
        Codegen::new().emit_program(&prog)
    }

    fn emit_at(level: StrictnessLevel, src: &str) -> String {
        let tokens = Lexer::new(src).tokenize().expect("tokenize");
        let prog = Parser::new(tokens).parse_program().expect("parse");
        Codegen::with_level(level).emit_program(&prog)
    }

    #[test]
    fn emits_struct_with_generic_param_and_clone_bound_at_explore() {
        let out = parse_and_emit("struct Box<T> { v: T }");
        assert!(out.contains("struct Box<T: Clone>"));
        assert!(out.contains("v: T"));
    }

    #[test]
    fn ship_drops_clone_bound_and_auto_derive() {
        let out = emit_at(StrictnessLevel::Ship, "struct Box<T> { v: T }");
        assert!(out.contains("struct Box<T>"));
        assert!(!out.contains("Clone, Debug, PartialEq"));
    }

    #[test]
    fn impl_with_generics_emits_both_brackets() {
        let out = parse_and_emit("impl<T> Foo<T> { fn new() -> Foo<T> { Foo {} } }");
        assert!(out.contains("impl<T: Clone> Foo<T>"));
    }

    #[test]
    fn impl_const_emits_typed_signature() {
        let out = parse_and_emit("struct Foo; impl Foo { const N: i64 = 42; }");
        assert!(out.contains("const N: i64 = 42;"));
    }

    #[test]
    fn fn_pointer_type_round_trips() {
        let out = parse_and_emit("fn apply(f: fn(i64) -> i64, x: i64) -> i64 { f(x) }");
        assert!(out.contains("f: fn(i64) -> i64"));
    }

    #[test]
    fn fn_trait_round_trips() {
        let out = parse_and_emit("fn apply(f: Fn(i64) -> i64) {}");
        assert!(out.contains("f: Fn(i64) -> i64"));
    }

    #[test]
    fn ref_with_lifetime_round_trips() {
        let out = parse_and_emit("fn name(s: &'static str) -> &'static str { s }");
        assert!(out.contains("&'static str"));
    }

    #[test]
    fn bare_ref_return_with_no_input_refs_promotes_to_static() {
        let out = parse_and_emit("fn give() -> &str { \"hi\" }");
        assert!(out.contains("&'static str"));
    }

    #[test]
    fn ref_return_with_input_ref_does_not_promote() {
        let out = parse_and_emit("fn first(xs: &Vec<i64>) -> &i64 { xs.first().unwrap() }");
        assert!(!out.contains("&'static i64"));
    }

    #[test]
    fn mut_self_emits_correctly() {
        let out = parse_and_emit("struct C; impl C { fn bump(&mut self) -> i64 { 1 } }");
        assert!(out.contains("&mut self"));
    }

    #[test]
    fn iter_chain_injects_cloned_at_explore() {
        let out = parse_and_emit(
            "fn main() { let v = vec![1, 2]; let _: Vec<i64> = v.iter().map(|x| x).collect(); }",
        );
        assert!(out.contains(".iter().cloned()"));
    }

    #[test]
    fn iter_chain_no_cloned_at_ship() {
        let out = emit_at(
            StrictnessLevel::Ship,
            "fn main() { let v = vec![1]; let _ = v.iter(); }",
        );
        assert!(!out.contains(".cloned()"));
    }

    #[test]
    fn turbofish_round_trips() {
        let out = parse_and_emit("fn main() { let _: i64 = vec![1].iter().sum::<i64>(); }");
        assert!(out.contains("sum::<i64>"));
    }

    #[test]
    fn range_in_method_receiver_is_parenthesised() {
        let out = parse_and_emit("fn main() { let _: i64 = (1..=10).sum(); }");
        assert!(out.contains("(1..=10).sum()"));
    }

    #[test]
    fn refutable_let_pattern_gets_else_unreachable() {
        let out = parse_and_emit(
            "fn main() { let xs = vec![1, 2, 3]; let [a, .., z] = xs.as_slice() else { return; }; }",
        );
        // The user's else block survives.
        assert!(out.contains("else"));
    }

    #[test]
    fn refutable_let_without_else_gets_synthesised() {
        let out = parse_and_emit("fn main() { let xs = vec![1, 2]; let [a, b] = xs.as_slice(); }");
        assert!(out.contains("else { unreachable!() }"));
    }

    #[test]
    fn user_supplied_derives_merge_with_auto() {
        let out = parse_and_emit("#[derive(Hash)] struct Foo { x: i64 }");
        // Author Hash gets merged with Clone, Debug, PartialEq.
        assert!(out.contains("Hash"));
        assert!(out.contains("Clone"));
    }

    #[test]
    fn inline_mod_pub_promotes_at_explore() {
        let out = parse_and_emit("mod m { fn h() -> i64 { 1 } } fn main() {}");
        assert!(out.contains("pub fn h"));
    }

    #[test]
    fn macro_call_round_trips() {
        let out = parse_and_emit("fn main() { println!(\"hi\"); }");
        assert!(out.contains("println!"));
    }

    #[test]
    fn allow_unused_header_present() {
        let out = parse_and_emit("fn main() {}");
        assert!(out.starts_with("#![allow"));
    }

    #[test]
    fn enum_with_variants_and_generics() {
        let out = parse_and_emit("enum Maybe<T> { Yes(T), No }");
        assert!(out.contains("enum Maybe<T: Clone>"));
        assert!(out.contains("Yes(T),"));
        assert!(out.contains("No,"));
    }

    #[test]
    fn trait_with_default_method() {
        let out = parse_and_emit("trait Greet { fn hi() -> i64 { 1 } }");
        assert!(out.contains("trait Greet"));
        assert!(out.contains("fn hi()"));
    }

    #[test]
    fn nested_module_emits_nested() {
        let out = parse_and_emit("mod outer { mod inner { fn f() {} } }");
        assert!(out.contains("mod outer"));
        assert!(out.contains("mod inner"));
    }

    #[test]
    fn pat_is_refutable_on_common_patterns() {
        // Smoke-test the helper used for let-else synthesis.
        assert!(!pat_is_refutable(&Pat::Wild));
        assert!(!pat_is_refutable(&Pat::Ident("x".into())));
        assert!(pat_is_refutable(&Pat::Lit(Lit::Int(5))));
        assert!(pat_is_refutable(&Pat::Tuple(vec![
            Pat::Ident("a".into()),
            Pat::Lit(Lit::Int(1)),
        ])));
    }

    #[test]
    fn chain_starts_with_iter_detects_root() {
        let tokens = Lexer::new("fn main() { let _ = vec![1].iter().map(|x| x); }")
            .tokenize()
            .unwrap();
        let prog = Parser::new(tokens).parse_program().unwrap();
        // Walk to find the .map() call and check its receiver.
        if let Item::Fn(f) = &prog[0] {
            for stmt in &f.body.stmts {
                if let Stmt::Let {
                    init: Some(Expr::MethodCall { receiver, .. }),
                    ..
                } = stmt
                {
                    assert!(chain_starts_with_iter(receiver));
                    return;
                }
            }
        }
        panic!("did not find expected method call");
    }

    #[test]
    fn iter_method_takes_owned_classifies_correctly() {
        assert!(iter_method_takes_owned("map"));
        assert!(iter_method_takes_owned("fold"));
        assert!(iter_method_takes_owned("any"));
        assert!(iter_method_takes_owned("all"));
        assert!(!iter_method_takes_owned("filter"));
        assert!(!iter_method_takes_owned("find"));
        assert!(!iter_method_takes_owned("max_by_key"));
    }
}
