use crate::ast::*;
use crate::error::{CrustError, Result};
use crate::lexer::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    pending_gt: bool, // leftover `>` after splitting `>>`
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            pos: 0,
            pending_gt: false,
        }
    }

    // ── Token navigation ──────────────────────────────────────────────────────

    fn peek(&self) -> &TokenKind {
        if self.pending_gt {
            return &TokenKind::Gt;
        }
        self.tokens
            .get(self.pos)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn peek_token(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn line(&self) -> usize {
        self.peek_token().line
    }

    fn advance(&mut self) -> &TokenKind {
        if self.pending_gt {
            self.pending_gt = false;
            return &TokenKind::Gt;
        }
        let k = &self.tokens[self.pos].kind;
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        k
    }

    fn check(&self, kind: &TokenKind) -> bool {
        self.peek() == kind
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.peek() == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<()> {
        if self.peek() == kind {
            self.advance();
            Ok(())
        } else {
            Err(CrustError::parse(
                format!("expected {:?}, got {:?}", kind, self.peek()),
                self.line(),
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.peek().clone() {
            TokenKind::Ident(s) => {
                self.advance();
                Ok(s)
            }
            TokenKind::SelfKw => {
                self.advance();
                Ok("self".to_string())
            }
            TokenKind::Underscore => {
                self.advance();
                Ok("_".to_string())
            }
            other => Err(CrustError::parse(
                format!("expected identifier, got {:?}", other),
                self.line(),
            )),
        }
    }

    // Read an identifier or qualified path (a::b::C), returning the last segment.
    fn expect_path_tail(&mut self) -> Result<String> {
        let mut name = self.expect_ident()?;
        while self.check(&TokenKind::ColonColon) {
            self.advance();
            name = self.expect_ident()?;
        }
        Ok(name)
    }

    /// Parse `<T, U, …>` and return the type-parameter *names*. Lifetime
    /// parameters (`'a`) and trait bounds (`T: Clone + Send`) are consumed
    /// but only the bare type-parameter names are returned. crust-1x4 will
    /// generalise this to a TyParam struct with bounds.
    fn parse_generic_params(&mut self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        if !self.check(&TokenKind::Lt) {
            return names;
        }
        self.advance(); // consume `<`
        let mut depth = 1i32;
        let mut expecting_name = true;
        while depth > 0 {
            match self.peek().clone() {
                TokenKind::Lt => {
                    depth += 1;
                    self.advance();
                }
                TokenKind::Gt => {
                    depth -= 1;
                    self.advance();
                    if depth == 0 {
                        break;
                    }
                }
                TokenKind::Shr => {
                    depth -= 2;
                    self.advance();
                    if depth <= 0 {
                        break;
                    }
                }
                TokenKind::Comma => {
                    self.advance();
                    expecting_name = true;
                }
                TokenKind::Ident(s) if expecting_name && depth == 1 => {
                    self.advance();
                    names.push(s);
                    expecting_name = false;
                }
                TokenKind::Eof => break,
                _ => {
                    self.advance();
                }
            }
        }
        names
    }

    // Skip generic parameters <T, U, ...> including lifetime params
    fn skip_generics(&mut self) {
        if !self.check(&TokenKind::Lt) {
            return;
        }
        let mut depth = 0i32;
        loop {
            match self.peek() {
                TokenKind::Lt => {
                    depth += 1;
                    self.advance();
                }
                TokenKind::Gt => {
                    depth -= 1;
                    self.advance();
                    if depth <= 0 {
                        break;
                    }
                }
                TokenKind::Shr => {
                    depth -= 2;
                    self.advance();
                    if depth <= 0 {
                        break;
                    }
                }
                TokenKind::Eof => break,
                _ => {
                    self.advance();
                }
            }
        }
    }

    // Skip where clause
    fn skip_where(&mut self) {
        if !self.check(&TokenKind::Where) {
            return;
        }
        self.advance();
        while !matches!(
            self.peek(),
            TokenKind::LBrace | TokenKind::Semi | TokenKind::Eof
        ) {
            self.advance();
        }
    }

    // ── Top-level ─────────────────────────────────────────────────────────────

    pub fn parse_program(&mut self) -> Result<Program> {
        let mut items = Vec::new();
        while !self.check(&TokenKind::Eof) {
            self.eat(&TokenKind::Semi); // stray semicolons
            if self.check(&TokenKind::Eof) {
                break;
            }
            items.push(self.parse_item()?);
        }
        Ok(items)
    }

    /// Drain any leading `Attr(…)` tokens into a `Vec<crate::ast::Attr>`.
    /// Unknown attributes (e.g. `derive(…)`, `allow(…)`) are stored verbatim
    /// so they can be re-emitted by the code generator.
    fn collect_attrs(&mut self) -> Vec<crate::ast::Attr> {
        let mut attrs = Vec::new();
        while let TokenKind::Attr(content) = self.peek().clone() {
            self.advance();
            attrs.push(parse_attr_content(&content));
        }
        attrs
    }

    fn parse_item(&mut self) -> Result<Item> {
        // Collect outer attributes: #[...]  (now emitted as Attr tokens by the lexer)
        let attrs = self.collect_attrs();

        // skip visibility
        self.eat(&TokenKind::Pub);

        // async fn
        if self.check(&TokenKind::Async) {
            self.advance();
            if !self.check(&TokenKind::Fn) {
                return Err(CrustError::parse(
                    "expected `fn` after `async`",
                    self.line(),
                ));
            }
            self.advance();
            let mut def = self.parse_fn_def()?;
            def.attrs = attrs;
            def.is_async = true;
            return Ok(Item::Fn(def));
        }

        match self.peek().clone() {
            TokenKind::Fn => {
                self.advance();
                let mut def = self.parse_fn_def()?;
                def.attrs = attrs;
                Ok(Item::Fn(def))
            }
            TokenKind::Struct => {
                self.advance();
                let mut def = self.parse_struct()?;
                def.attrs = attrs;
                Ok(Item::Struct(def))
            }
            TokenKind::Enum => {
                self.advance();
                let mut def = self.parse_enum()?;
                def.attrs = attrs;
                Ok(Item::Enum(def))
            }
            TokenKind::Impl => {
                self.advance();
                Ok(Item::Impl(self.parse_impl()?))
            }
            TokenKind::Use => {
                self.advance();
                Ok(Item::Use(self.parse_use()?))
            }
            TokenKind::Const => {
                self.advance();
                // `const fn` → treat as regular function
                if self.check(&TokenKind::Fn) {
                    self.advance();
                    let mut def = self.parse_fn_def()?;
                    def.attrs = attrs;
                    Ok(Item::Fn(def))
                } else {
                    self.parse_const()
                }
            }
            TokenKind::Type => {
                self.advance();
                self.parse_type_alias()
            }
            TokenKind::Static => {
                self.advance();
                self.eat(&TokenKind::Mut);
                self.parse_const()
            }
            TokenKind::Trait => {
                self.advance();
                self.parse_trait()
            }
            TokenKind::Mod => {
                self.advance();
                self.parse_mod()
            }
            other => Err(CrustError::parse(
                format!("unexpected token at item level: {:?}", other),
                self.line(),
            )),
        }
    }

    /// `mod NAME { items }` — inline module. File-based `mod foo;` is rejected
    /// here with a clear diagnostic so callers know it's not yet implemented.
    fn parse_mod(&mut self) -> Result<Item> {
        let name = self.expect_ident()?;
        if self.eat(&TokenKind::Semi) {
            return Err(CrustError::parse(
                format!(
                    "`mod {};` (file-based module) is not supported yet; \
                     use inline `mod {} {{ ... }}` for now",
                    name, name
                ),
                self.line(),
            ));
        }
        self.expect(&TokenKind::LBrace)?;
        let mut items = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            self.eat(&TokenKind::Semi);
            if self.check(&TokenKind::RBrace) {
                break;
            }
            items.push(self.parse_item()?);
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Item::Mod { name, items })
    }

    fn parse_fn_def(&mut self) -> Result<FnDef> {
        let name = self.expect_ident()?;
        let generics = self.parse_generic_params();
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen)?;
        let ret_ty = if self.eat(&TokenKind::Arrow) {
            Some(self.parse_ty()?)
        } else {
            None
        };
        self.skip_where();
        let body = self.parse_block()?;
        Ok(FnDef {
            generics,
            name,
            params,
            ret_ty,
            body,
            attrs: vec![],
            is_async: false,
        })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>> {
        let mut params = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
            // &self, &mut self, self, mut self
            let mutable = self.eat(&TokenKind::Mut);
            if self.check(&TokenKind::And) {
                self.advance();
                let ref_mut = self.eat(&TokenKind::Mut);
                if self.check(&TokenKind::SelfKw) {
                    self.advance();
                    params.push(Param {
                        name: "self".into(),
                        ty: Ty::Ref(ref_mut, Box::new(Ty::Named("Self".into()))),
                        is_self: true,
                        mutable: ref_mut,
                    });
                } else {
                    // &Type param
                    let name = self.expect_ident()?;
                    self.expect(&TokenKind::Colon)?;
                    let ty = self.parse_ty()?;
                    params.push(Param {
                        name,
                        ty,
                        is_self: false,
                        mutable,
                    });
                }
            } else if self.check(&TokenKind::SelfKw) {
                self.advance();
                params.push(Param {
                    name: "self".into(),
                    ty: Ty::Named("Self".into()),
                    is_self: true,
                    mutable,
                });
            } else if matches!(self.peek(), TokenKind::Ident(_)) {
                let name = self.expect_ident()?;
                if self.eat(&TokenKind::Colon) {
                    let ty = self.parse_ty()?;
                    params.push(Param {
                        name,
                        ty,
                        is_self: false,
                        mutable,
                    });
                } else {
                    // bare name, use Unit type (shouldn't happen in valid code)
                    params.push(Param {
                        name,
                        ty: Ty::Unit,
                        is_self: false,
                        mutable,
                    });
                }
            } else if matches!(self.peek(), TokenKind::Underscore) {
                self.advance();
                self.expect(&TokenKind::Colon)?;
                let _ty = self.parse_ty()?;
                // ignore underscore params
            } else {
                break;
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        Ok(params)
    }

    fn parse_struct(&mut self) -> Result<StructDef> {
        let name = self.expect_ident()?;
        let generics = self.parse_generic_params();
        self.skip_where();
        if self.eat(&TokenKind::LParen) {
            // Tuple struct: struct Foo(T1, T2);
            let mut fields = Vec::new();
            let mut idx = 0usize;
            while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                self.eat(&TokenKind::Pub);
                let ty = self.parse_ty()?;
                fields.push((idx.to_string(), ty));
                idx += 1;
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::RParen)?;
            self.eat(&TokenKind::Semi);
            return Ok(StructDef {
                name,
                fields,
                attrs: Vec::new(),
                generics,
            });
        }
        // Unit struct: struct Foo;
        if self.eat(&TokenKind::Semi) {
            return Ok(StructDef {
                name,
                fields: Vec::new(),
                attrs: Vec::new(),
                generics,
            });
        }
        self.expect(&TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            self.eat(&TokenKind::Pub);
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let ty = self.parse_ty()?;
            fields.push((fname, ty));
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(StructDef {
            name,
            fields,
            attrs: Vec::new(),
            generics,
        })
    }

    fn parse_enum(&mut self) -> Result<EnumDef> {
        let name = self.expect_ident()?;
        let generics = self.parse_generic_params();
        self.skip_where();
        self.expect(&TokenKind::LBrace)?;
        let mut variants = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            self.eat(&TokenKind::Pub);
            let vname = self.expect_ident()?;
            let data = if self.check(&TokenKind::LBrace) {
                self.advance();
                let mut fields = Vec::new();
                while !self.check(&TokenKind::RBrace) {
                    self.eat(&TokenKind::Pub);
                    let n = self.expect_ident()?;
                    self.expect(&TokenKind::Colon)?;
                    let t = self.parse_ty()?;
                    fields.push((n, t));
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RBrace)?;
                VariantData::Struct(fields)
            } else if self.check(&TokenKind::LParen) {
                self.advance();
                let mut tys = Vec::new();
                while !self.check(&TokenKind::RParen) {
                    tys.push(self.parse_ty()?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen)?;
                VariantData::Tuple(tys)
            } else {
                VariantData::Unit
            };
            // skip discriminant = value
            if self.eat(&TokenKind::Eq) {
                self.parse_expr(0)?;
            }
            variants.push(EnumVariant { name: vname, data });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(EnumDef {
            name,
            variants,
            attrs: Vec::new(),
            generics,
        })
    }

    fn parse_impl(&mut self) -> Result<ImplDef> {
        // `impl<T, U>` introduces names; `impl Foo<T>` and `impl Trait<T> for Bar<T>`
        // apply names to the implementing type. Capture both for codegen.
        let generics = self.parse_generic_params();
        let first = self.expect_path_tail()?;
        let first_args = self.parse_generic_params();
        let (type_name, trait_name, type_args) = if self.eat(&TokenKind::For) {
            let ty = self.expect_path_tail()?;
            let impl_args = self.parse_generic_params();
            (ty, Some(first), impl_args)
        } else {
            (first, None, first_args)
        };
        self.skip_where();
        self.expect(&TokenKind::LBrace)?;
        let mut methods = Vec::new();
        let mut consts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let attrs = self.collect_attrs();
            self.eat(&TokenKind::Pub);
            let is_async = self.eat(&TokenKind::Async);
            if self.check(&TokenKind::Fn) {
                self.advance();
                let mut def = self.parse_fn_def()?;
                def.attrs = attrs;
                def.is_async = is_async;
                methods.push(def);
            } else if self.check(&TokenKind::Const) {
                self.advance(); // consume `const`
                let const_name = self.expect_ident()?;
                // Capture the declared type so codegen can emit
                // `const NAME: TY = ...;` (rustc rejects `_` placeholder for
                // associated consts — E0121).  Default to `i64` if missing.
                let const_ty = if self.eat(&TokenKind::Colon) {
                    self.parse_ty()?
                } else {
                    Ty::Named("i64".to_string())
                };
                self.expect(&TokenKind::Eq)?;
                let val = self.parse_expr(0)?;
                self.eat(&TokenKind::Semi);
                consts.push((const_name, const_ty, val));
            } else if self.check(&TokenKind::Type) {
                // skip type aliases
                while !matches!(
                    self.peek(),
                    TokenKind::Semi | TokenKind::RBrace | TokenKind::Eof
                ) {
                    self.advance();
                }
                self.eat(&TokenKind::Semi);
            } else {
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(ImplDef {
            type_name,
            trait_name,
            methods,
            consts,
            generics,
            type_args,
        })
    }

    fn parse_trait(&mut self) -> Result<Item> {
        let name = self.expect_ident()?;
        let generics = self.parse_generic_params();
        // optional supertrait bounds: trait Foo: Bar + Baz
        if self.eat(&TokenKind::Colon) {
            while !self.check(&TokenKind::LBrace) && !self.check(&TokenKind::Eof) {
                self.advance();
            }
        }
        self.skip_where();
        self.expect(&TokenKind::LBrace)?;
        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let attrs = self.collect_attrs();
            self.eat(&TokenKind::Pub);
            let is_async = self.eat(&TokenKind::Async);
            if self.check(&TokenKind::Fn) {
                self.advance();
                let fn_name = self.expect_ident()?;
                let fn_generics = self.parse_generic_params();
                self.expect(&TokenKind::LParen)?;
                let params = self.parse_params()?;
                self.expect(&TokenKind::RParen)?;
                let ret_ty = if self.eat(&TokenKind::Arrow) {
                    Some(self.parse_ty()?)
                } else {
                    None
                };
                self.skip_where();
                if self.check(&TokenKind::LBrace) {
                    // default method body
                    let body = self.parse_block()?;
                    methods.push(FnDef {
                        name: fn_name,
                        params,
                        ret_ty,
                        body,
                        attrs,
                        is_async,
                        generics: fn_generics,
                    });
                } else {
                    // required method (no body) — skip the semicolon
                    self.eat(&TokenKind::Semi);
                }
            } else if matches!(self.peek(), TokenKind::Const | TokenKind::Type) {
                while !matches!(
                    self.peek(),
                    TokenKind::Semi | TokenKind::RBrace | TokenKind::Eof
                ) {
                    self.advance();
                }
                self.eat(&TokenKind::Semi);
            } else {
                self.advance(); // skip unknown tokens inside trait
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Item::Trait {
            name,
            methods,
            generics,
        })
    }

    fn parse_use(&mut self) -> Result<Vec<String>> {
        let mut path = Vec::new();
        loop {
            match self.peek().clone() {
                TokenKind::Ident(s) => {
                    self.advance();
                    path.push(s);
                }
                TokenKind::SelfKw => {
                    self.advance();
                    path.push("self".into());
                }
                TokenKind::LBrace => {
                    // use foo::{a, b} — skip brace group
                    self.advance();
                    while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
                        self.advance();
                    }
                    self.eat(&TokenKind::RBrace);
                    break;
                }
                TokenKind::Star => {
                    self.advance();
                    path.push("*".into());
                    break;
                }
                _ => break,
            }
            if !self.eat(&TokenKind::ColonColon) {
                break;
            }
        }
        self.eat(&TokenKind::Semi);
        Ok(path)
    }

    fn parse_const(&mut self) -> Result<Item> {
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        let ty = self.parse_ty()?;
        self.expect(&TokenKind::Eq)?;
        let value = self.parse_expr(0)?;
        self.eat(&TokenKind::Semi);
        Ok(Item::Const { name, ty, value })
    }

    fn parse_type_alias(&mut self) -> Result<Item> {
        let name = self.expect_ident()?;
        self.skip_generics();
        self.expect(&TokenKind::Eq)?;
        let ty = self.parse_ty()?;
        self.eat(&TokenKind::Semi);
        Ok(Item::TypeAlias { name, ty })
    }

    // ── Types ─────────────────────────────────────────────────────────────────

    fn parse_ty(&mut self) -> Result<Ty> {
        match self.peek().clone() {
            TokenKind::LParen => {
                self.advance();
                if self.eat(&TokenKind::RParen) {
                    return Ok(Ty::Unit);
                }
                let first = self.parse_ty()?;
                if self.eat(&TokenKind::Comma) {
                    let mut tys = vec![first];
                    while !self.check(&TokenKind::RParen) {
                        tys.push(self.parse_ty()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Ty::Tuple(tys))
                } else {
                    self.expect(&TokenKind::RParen)?;
                    Ok(first)
                }
            }
            TokenKind::Not => {
                self.advance();
                Ok(Ty::Never)
            }
            TokenKind::And | TokenKind::AndAnd => {
                // `&&T` or `&T` — both become a single ref at Level 0.
                // Capture the lifetime annotation if present so codegen can
                // re-emit it; without this the elided form `&str` would
                // hit E0106 on bare returns (crust-1x4).
                self.advance();
                let lifetime = if let TokenKind::Label(name) = self.peek().clone() {
                    self.advance();
                    Some(name)
                } else {
                    None
                };
                let mutable = self.eat(&TokenKind::Mut);
                let inner = self.parse_ty()?;
                if let Some(lt) = lifetime {
                    Ok(Ty::RefLt(mutable, lt, Box::new(inner)))
                } else {
                    Ok(Ty::Ref(mutable, Box::new(inner)))
                }
            }
            TokenKind::Star => {
                self.advance();
                let mutable = self.eat(&TokenKind::Mut);
                if !mutable {
                    self.eat(&TokenKind::Const);
                }
                let inner = self.parse_ty()?;
                Ok(Ty::Ptr(mutable, Box::new(inner)))
            }
            TokenKind::LBracket => {
                self.advance();
                let inner = self.parse_ty()?;
                if self.eat(&TokenKind::Semi) {
                    // [T; N] - array, treat as slice
                    while !self.check(&TokenKind::RBracket) && !self.check(&TokenKind::Eof) {
                        self.advance();
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(Ty::Slice(Box::new(inner)))
            }
            TokenKind::Fn | TokenKind::Ident(_) | TokenKind::SelfKw => {
                // fn(T, ...) -> R — record params and return type as Ty::FnPtr
                // so codegen can re-emit a valid Rust function pointer type.
                if self.check(&TokenKind::Fn) {
                    self.advance();
                    let mut params: Vec<Ty> = Vec::new();
                    if self.eat(&TokenKind::LParen) {
                        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                            params.push(self.parse_ty()?);
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                        }
                        self.eat(&TokenKind::RParen);
                    }
                    let ret = if self.eat(&TokenKind::Arrow) {
                        self.parse_ty()?
                    } else {
                        Ty::Unit
                    };
                    return Ok(Ty::FnPtr {
                        kind: String::new(),
                        params,
                        ret: Box::new(ret),
                    });
                }
                // could be a path like std::string::String
                let name = match self.peek().clone() {
                    TokenKind::SelfKw => {
                        self.advance();
                        "Self".to_string()
                    }
                    _ => self.expect_ident()?,
                };
                // skip ::Name chains
                while self.eat(&TokenKind::ColonColon) {
                    match self.peek().clone() {
                        TokenKind::Ident(s) => {
                            self.advance();
                            let _ = s;
                        }
                        TokenKind::Lt => {
                            self.skip_generics();
                        }
                        _ => {}
                    }
                }
                // dyn Trait — consume dyn, parse the following type
                if name == "dyn" {
                    return self.parse_ty();
                }
                // Fn(T) -> R / FnMut / FnOnce — capture as Ty::FnPtr so codegen
                // can emit `Fn(T) -> R` rather than the bare token "fn".
                if matches!(name.as_str(), "Fn" | "FnMut" | "FnOnce")
                    && self.check(&TokenKind::LParen)
                {
                    self.eat(&TokenKind::LParen);
                    let mut params: Vec<Ty> = Vec::new();
                    while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                        params.push(self.parse_ty()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.eat(&TokenKind::RParen);
                    let ret = if self.eat(&TokenKind::Arrow) {
                        self.parse_ty()?
                    } else {
                        Ty::Unit
                    };
                    return Ok(Ty::FnPtr {
                        kind: name,
                        params,
                        ret: Box::new(ret),
                    });
                }
                if self.check(&TokenKind::Lt) {
                    self.advance();
                    let mut args = Vec::new();
                    while !self.check(&TokenKind::Gt)
                        && !matches!(self.peek(), TokenKind::Shr | TokenKind::Eof)
                    {
                        // Skip lifetime parameters like `'a`
                        if matches!(self.peek(), TokenKind::Label(_)) {
                            self.advance();
                            self.eat(&TokenKind::Comma);
                            continue;
                        }
                        args.push(self.parse_ty()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    if !self.eat(&TokenKind::Gt) {
                        // `>>` closes this generic, leave a pending `>` for the parent
                        if self.eat(&TokenKind::Shr) {
                            self.pending_gt = true;
                        }
                    }
                    Ok(Ty::Generic(name, args))
                } else {
                    Ok(Ty::Named(name))
                }
            }
            TokenKind::Impl => {
                // impl Trait + Bound + 'static — skip all and treat as Named("impl")
                self.advance();
                let _ = self.parse_ty()?; // consume the primary trait type
                                          // consume additional trait bounds: + Trait + 'lifetime
                while self.check(&TokenKind::Plus) {
                    self.advance(); // consume +
                                    // skip lifetime bound like 'static
                    if matches!(self.peek(), TokenKind::Label(_)) {
                        self.advance();
                    } else if matches!(self.peek(), TokenKind::Ident(_)) {
                        let _ = self.parse_ty()?;
                    }
                }
                Ok(Ty::Named("impl".to_string()))
            }
            TokenKind::Underscore => {
                self.advance();
                Ok(Ty::Named("_".to_string()))
            }
            other => Err(CrustError::parse(
                format!("expected type, got {:?}", other),
                self.line(),
            )),
        }
    }

    // ── Statements ────────────────────────────────────────────────────────────

    fn parse_block(&mut self) -> Result<Block> {
        self.expect(&TokenKind::LBrace)?;
        let mut stmts = Vec::new();
        let mut tail: Option<Box<Expr>> = None;

        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            // item-level declarations inside blocks (including async fn and attributed fns)
            if matches!(
                self.peek(),
                TokenKind::Fn
                    | TokenKind::Struct
                    | TokenKind::Enum
                    | TokenKind::Impl
                    | TokenKind::Use
                    | TokenKind::Const
                    | TokenKind::Static
                    | TokenKind::Async
                    | TokenKind::Attr(_)
            ) || (self.check(&TokenKind::Pub) && {
                let saved = self.pos;
                self.advance();
                let is_item = matches!(
                    self.peek(),
                    TokenKind::Fn
                        | TokenKind::Struct
                        | TokenKind::Enum
                        | TokenKind::Impl
                        | TokenKind::Async
                );
                self.pos = saved;
                is_item
            }) {
                let item = self.parse_item()?;
                stmts.push(Stmt::Item(item));
                continue;
            }

            if self.check(&TokenKind::Let) {
                stmts.push(self.parse_let()?);
                continue;
            }

            let expr = self.parse_expr(0)?;

            if self.eat(&TokenKind::Semi) {
                stmts.push(Stmt::Semi(expr));
            } else if self.check(&TokenKind::RBrace) {
                tail = Some(Box::new(expr));
                break;
            } else {
                // block expression without semicolon inside a block
                stmts.push(Stmt::Expr(expr));
            }
        }

        self.expect(&TokenKind::RBrace)?;
        Ok(Block { stmts, tail })
    }

    fn parse_let(&mut self) -> Result<Stmt> {
        self.advance(); // consume `let`
        let mutable = self.eat(&TokenKind::Mut);
        // Tuple / struct destructuring pattern: let (a, b) = ...
        if self.check(&TokenKind::LParen) {
            let pat = self.parse_pat()?;
            let ty = if self.eat(&TokenKind::Colon) {
                Some(self.parse_ty()?)
            } else {
                None
            };
            let init = if self.eat(&TokenKind::Eq) {
                Some(self.parse_expr(0)?)
            } else {
                None
            };
            let else_block = if self.eat(&TokenKind::Else) {
                Some(self.parse_block()?)
            } else {
                None
            };
            self.eat(&TokenKind::Semi);
            return Ok(Stmt::LetPat {
                pat,
                ty,
                init,
                else_block,
            });
        }
        // Slice pattern: let [a, b, ..] = ...
        if self.check(&TokenKind::LBracket) {
            let pat = self.parse_pat_single()?;
            let ty = if self.eat(&TokenKind::Colon) {
                Some(self.parse_ty()?)
            } else {
                None
            };
            let init = if self.eat(&TokenKind::Eq) {
                Some(self.parse_expr(0)?)
            } else {
                None
            };
            let else_block = if self.eat(&TokenKind::Else) {
                Some(self.parse_block()?)
            } else {
                None
            };
            self.eat(&TokenKind::Semi);
            return Ok(Stmt::LetPat {
                pat,
                ty,
                init,
                else_block,
            });
        }
        let name = match self.peek().clone() {
            TokenKind::Ident(s) => {
                self.advance();
                s
            }
            TokenKind::Underscore => {
                self.advance();
                "_".to_string()
            }
            other => {
                return Err(CrustError::parse(
                    format!("expected pattern in let, got {:?}", other),
                    self.line(),
                ))
            }
        };
        // Struct pattern: let Point { x, y } = ... or let Ns::Type { ... } = ...
        // Detect if this is a struct-pattern let by checking for `{` possibly after `::` path
        let mut path = vec![name.clone()];
        while self.check(&TokenKind::ColonColon) {
            let saved_pos = self.pos;
            self.advance(); // consume ::
            match self.peek().clone() {
                TokenKind::Ident(s) => {
                    self.advance();
                    path.push(s);
                }
                _ => {
                    self.pos = saved_pos;
                    break;
                }
            }
        }
        // Tuple struct pattern: let Rgb(r, g, b) = ...
        if self.check(&TokenKind::LParen) {
            let full_name = path.join("::");
            self.advance(); // consume (
            let mut pats = Vec::new();
            while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                pats.push(self.parse_pat_single()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            self.eat(&TokenKind::RParen);
            let pat = Pat::TupleStruct {
                name: full_name,
                fields: pats,
            };
            let ty = if self.eat(&TokenKind::Colon) {
                Some(self.parse_ty()?)
            } else {
                None
            };
            let init = if self.eat(&TokenKind::Eq) {
                Some(self.parse_expr(0)?)
            } else {
                None
            };
            let else_block = if self.eat(&TokenKind::Else) {
                Some(self.parse_block()?)
            } else {
                None
            };
            self.eat(&TokenKind::Semi);
            return Ok(Stmt::LetPat {
                pat,
                ty,
                init,
                else_block,
            });
        }
        if self.check(&TokenKind::LBrace) {
            // struct destructuring pattern
            let full_name = path.join("::");
            let saved_pos = self.pos;
            self.advance(); // consume {
            let mut fields = Vec::new();
            let mut rest = false;
            let mut valid = true;
            while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
                if self.eat(&TokenKind::DotDot) {
                    rest = true;
                    break;
                }
                let fname = match self.peek().clone() {
                    TokenKind::Ident(s) => {
                        self.advance();
                        s
                    }
                    _ => {
                        valid = false;
                        break;
                    }
                };
                let fpat = if self.eat(&TokenKind::Colon) {
                    self.parse_pat()?
                } else {
                    Pat::Ident(fname.clone())
                };
                fields.push((fname, fpat));
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            if valid && self.eat(&TokenKind::RBrace) {
                let pat = Pat::Struct {
                    name: full_name,
                    fields,
                    rest,
                };
                let ty = if self.eat(&TokenKind::Colon) {
                    Some(self.parse_ty()?)
                } else {
                    None
                };
                let init = if self.eat(&TokenKind::Eq) {
                    Some(self.parse_expr(0)?)
                } else {
                    None
                };
                let else_block = if self.eat(&TokenKind::Else) {
                    Some(self.parse_block()?)
                } else {
                    None
                };
                self.eat(&TokenKind::Semi);
                return Ok(Stmt::LetPat {
                    pat,
                    ty,
                    init,
                    else_block,
                });
            }
            // Not a valid struct pattern, reset and fall through
            self.pos = saved_pos;
        } else if path.len() > 1 {
            // reset path parsing (no struct brace followed)
            // we consumed extra tokens, need to back up — actually just use path.join("::")
        }
        // Simple identifier let (path.len() == 1, no struct brace)
        if path.len() > 1 {
            // e.g. `let Foo::Bar = ...` — unlikely but handle as ident
        }
        let name = if path.len() == 1 {
            path.into_iter().next().unwrap()
        } else {
            path.join("::")
        };
        let ty = if self.eat(&TokenKind::Colon) {
            Some(self.parse_ty()?)
        } else {
            None
        };
        let init = if self.eat(&TokenKind::Eq) {
            Some(self.parse_expr(0)?)
        } else {
            None
        };
        // let-else: let PATTERN = EXPR else { BLOCK }
        if self.check(&TokenKind::Else) {
            self.advance();
            let else_block = self.parse_block()?;
            let pat = Pat::Ident(name);
            self.eat(&TokenKind::Semi);
            return Ok(Stmt::LetPat {
                pat,
                ty,
                init,
                else_block: Some(else_block),
            });
        }
        self.eat(&TokenKind::Semi);
        Ok(Stmt::Let {
            name,
            mutable,
            ty,
            init,
        })
    }

    // ── Expressions (recursive descent with precedence) ───────────────────────

    pub fn parse_expr(&mut self, min_prec: u8) -> Result<Expr> {
        let mut lhs = self.parse_unary()?;

        loop {
            // for/while/loop always return () — don't let them grab trailing operators
            if is_block_stmt_expr(&lhs) {
                break;
            }

            let (op, prec, right_assoc) = match self.peek() {
                // assignment (right-assoc, lowest)
                TokenKind::Eq => (None::<BinOp>, 1, true),
                TokenKind::PlusEq => (Some(BinOp::Add), 1, true),
                TokenKind::MinusEq => (Some(BinOp::Sub), 1, true),
                TokenKind::StarEq => (Some(BinOp::Mul), 1, true),
                TokenKind::SlashEq => (Some(BinOp::Div), 1, true),
                TokenKind::PercentEq => (Some(BinOp::Rem), 1, true),
                TokenKind::AndEq => (Some(BinOp::BitAnd), 1, true),
                TokenKind::OrEq => (Some(BinOp::BitOr), 1, true),
                TokenKind::CaretEq => (Some(BinOp::BitXor), 1, true),
                TokenKind::ShlEq => (Some(BinOp::Shl), 1, true),
                TokenKind::ShrEq => (Some(BinOp::Shr), 1, true),
                // range (right-assoc)
                TokenKind::DotDot => (None, 2, true),
                TokenKind::DotDotEq => (None, 2, true),
                // binary ops (left-assoc)
                TokenKind::OrOr => (Some(BinOp::Or), 3, false),
                TokenKind::AndAnd => (Some(BinOp::And), 4, false),
                TokenKind::EqEq => (Some(BinOp::Eq), 5, false),
                TokenKind::Ne => (Some(BinOp::Ne), 5, false),
                TokenKind::Lt => (Some(BinOp::Lt), 5, false),
                TokenKind::Le => (Some(BinOp::Le), 5, false),
                TokenKind::Gt => (Some(BinOp::Gt), 5, false),
                TokenKind::Ge => (Some(BinOp::Ge), 5, false),
                TokenKind::Or => (Some(BinOp::BitOr), 6, false),
                TokenKind::Caret => (Some(BinOp::BitXor), 7, false),
                TokenKind::And => (Some(BinOp::BitAnd), 8, false),
                TokenKind::Shl => (Some(BinOp::Shl), 9, false),
                TokenKind::Shr => (Some(BinOp::Shr), 9, false),
                TokenKind::Plus => (Some(BinOp::Add), 10, false),
                TokenKind::Minus => (Some(BinOp::Sub), 10, false),
                TokenKind::Star => (Some(BinOp::Mul), 11, false),
                TokenKind::Slash => (Some(BinOp::Div), 11, false),
                TokenKind::Percent => (Some(BinOp::Rem), 11, false),
                // `as` cast
                TokenKind::As => (None, 12, false),
                _ => break,
            };

            if prec < min_prec {
                break;
            }
            let tok = self.advance().clone();

            match &tok {
                TokenKind::Eq => {
                    let rhs = self.parse_expr(prec)?;
                    lhs = Expr::Assign(Box::new(lhs), Box::new(rhs));
                }
                TokenKind::PlusEq
                | TokenKind::MinusEq
                | TokenKind::StarEq
                | TokenKind::SlashEq
                | TokenKind::PercentEq
                | TokenKind::AndEq
                | TokenKind::OrEq
                | TokenKind::CaretEq
                | TokenKind::ShlEq
                | TokenKind::ShrEq => {
                    let binop = op.unwrap();
                    let rhs = self.parse_expr(prec)?;
                    lhs = Expr::OpAssign(binop, Box::new(lhs), Box::new(rhs));
                }
                TokenKind::DotDot => {
                    let inclusive = false;
                    let rhs = if !matches!(
                        self.peek(),
                        TokenKind::Semi
                            | TokenKind::RBracket
                            | TokenKind::RParen
                            | TokenKind::RBrace
                            | TokenKind::Comma
                            | TokenKind::Eof
                    ) {
                        Some(Box::new(self.parse_expr(prec + 1)?))
                    } else {
                        None
                    };
                    lhs = Expr::Range {
                        start: Some(Box::new(lhs)),
                        end: rhs,
                        inclusive,
                    };
                }
                TokenKind::DotDotEq => {
                    let rhs = self.parse_expr(prec + 1)?;
                    lhs = Expr::Range {
                        start: Some(Box::new(lhs)),
                        end: Some(Box::new(rhs)),
                        inclusive: true,
                    };
                }
                TokenKind::As => {
                    let ty = self.parse_ty()?;
                    lhs = Expr::Cast(Box::new(lhs), ty);
                }
                _ => {
                    let binop = op.unwrap();
                    let next_prec = if right_assoc { prec } else { prec + 1 };
                    let rhs = self.parse_expr(next_prec)?;
                    lhs = Expr::Binary(binop, Box::new(lhs), Box::new(rhs));
                }
            }
        }

        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            TokenKind::Minus => {
                self.advance();
                // peek: if next is a number literal, fold into negative literal
                match self.peek().clone() {
                    TokenKind::Int(n) => {
                        self.advance();
                        Ok(Expr::Lit(Lit::Int(-n)))
                    }
                    TokenKind::Float(f) => {
                        self.advance();
                        Ok(Expr::Lit(Lit::Float(-f)))
                    }
                    _ => Ok(Expr::Unary(UnOp::Neg, Box::new(self.parse_unary()?))),
                }
            }
            TokenKind::Not => {
                self.advance();
                Ok(Expr::Unary(UnOp::Not, Box::new(self.parse_unary()?)))
            }
            TokenKind::Star => {
                self.advance();
                Ok(Expr::Deref(Box::new(self.parse_unary()?)))
            }
            TokenKind::And => {
                self.advance();
                let mutable = self.eat(&TokenKind::Mut);
                Ok(Expr::Ref {
                    mutable,
                    expr: Box::new(self.parse_unary()?),
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek().clone() {
                TokenKind::Dot => {
                    self.advance();
                    match self.peek().clone() {
                        TokenKind::Int(n) => {
                            // tuple field access: t.0
                            self.advance();
                            expr = Expr::Field(Box::new(expr), n.to_string());
                        }
                        _ => {
                            let field = self.expect_ident()?;
                            // `.await` — postfix await operator
                            if field == "await" {
                                expr = Expr::Await(Box::new(expr));
                                continue;
                            }
                            // turbofish: expr.method::<T1, T2>(args) — collect a real
                            // Vec<Ty> so codegen can re-emit the full annotation.
                            let turbofish = if self.check(&TokenKind::ColonColon) {
                                self.advance();
                                if self.check(&TokenKind::Lt) {
                                    self.advance(); // consume <
                                    let mut tys: Vec<Ty> = Vec::new();
                                    while !self.check(&TokenKind::Gt)
                                        && !matches!(self.peek(), TokenKind::Shr | TokenKind::Eof)
                                    {
                                        if matches!(self.peek(), TokenKind::Label(_)) {
                                            self.advance();
                                            self.eat(&TokenKind::Comma);
                                            continue;
                                        }
                                        tys.push(self.parse_ty()?);
                                        if !self.eat(&TokenKind::Comma) {
                                            break;
                                        }
                                    }
                                    if !self.eat(&TokenKind::Gt) && self.eat(&TokenKind::Shr) {
                                        self.pending_gt = true;
                                    }
                                    Some(tys)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            if self.check(&TokenKind::LParen) {
                                self.advance();
                                let args = self.parse_args()?;
                                self.expect(&TokenKind::RParen)?;
                                expr = Expr::MethodCall {
                                    receiver: Box::new(expr),
                                    method: field,
                                    turbofish,
                                    args,
                                };
                            } else {
                                expr = Expr::Field(Box::new(expr), field);
                            }
                        }
                    }
                }
                TokenKind::LBracket => {
                    self.advance();
                    let idx = self.parse_expr(0)?;
                    self.expect(&TokenKind::RBracket)?;
                    expr = Expr::Index(Box::new(expr), Box::new(idx));
                }
                TokenKind::LParen => {
                    // Don't treat block-like exprs followed by `(` as function calls.
                    // e.g. `while {...} (2..=n).filter(...)` must NOT parse as `while_result(2..=n)`.
                    let is_block_like = matches!(
                        &expr,
                        Expr::Block(_)
                            | Expr::Unsafe(_)
                            | Expr::If { .. }
                            | Expr::Match { .. }
                            | Expr::Macro { .. }
                    );
                    if is_block_like {
                        break;
                    }
                    self.advance();
                    let args = self.parse_args()?;
                    self.expect(&TokenKind::RParen)?;
                    expr = Expr::Call {
                        func: Box::new(expr),
                        args,
                    };
                }
                TokenKind::Question => {
                    self.advance();
                    expr = Expr::Try(Box::new(expr));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>> {
        let mut args = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
            args.push(self.parse_expr(0)?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            // Literals
            TokenKind::Int(n) => {
                self.advance();
                Ok(Expr::Lit(Lit::Int(n)))
            }
            TokenKind::Float(f) => {
                self.advance();
                Ok(Expr::Lit(Lit::Float(f)))
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::Lit(Lit::Bool(true)))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::Lit(Lit::Bool(false)))
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Expr::Lit(Lit::Str(s)))
            }
            TokenKind::Char(c) => {
                self.advance();
                Ok(Expr::Lit(Lit::Char(c)))
            }

            // Macro call
            TokenKind::MacroName(name) => {
                self.advance();
                // matches!(expr, pat1 | pat2 | ...) → match expr { pat => true, _ => false }
                if name == "matches" {
                    self.expect(&TokenKind::LParen)?;
                    let scrutinee = self.parse_expr(0)?;
                    self.expect(&TokenKind::Comma)?;
                    let mut pats = vec![self.parse_pat()?];
                    while self.eat(&TokenKind::Or) {
                        pats.push(self.parse_pat()?);
                    }
                    // optional guard: if <expr>
                    let guard = if self.eat(&TokenKind::If) {
                        Some(self.parse_expr(0)?)
                    } else {
                        None
                    };
                    self.expect(&TokenKind::RParen)?;
                    let pat = if pats.len() == 1 {
                        pats.remove(0)
                    } else {
                        Pat::Or(pats)
                    };
                    return Ok(Expr::Match {
                        scrutinee: Box::new(scrutinee),
                        arms: vec![
                            crate::ast::MatchArm {
                                pat,
                                guard,
                                body: Expr::Lit(Lit::Bool(true)),
                            },
                            crate::ast::MatchArm {
                                pat: Pat::Wild,
                                guard: None,
                                body: Expr::Lit(Lit::Bool(false)),
                            },
                        ],
                    });
                }
                let (open, close) = if self.check(&TokenKind::LParen) {
                    (TokenKind::LParen, TokenKind::RParen)
                } else if self.check(&TokenKind::LBracket) {
                    (TokenKind::LBracket, TokenKind::RBracket)
                } else {
                    (TokenKind::LBrace, TokenKind::RBrace)
                };
                self.expect(&open)?;
                let args = if close == TokenKind::RParen {
                    self.parse_macro_args()?
                } else {
                    // vec![a, b, c] or vec![value; count]
                    let mut args = Vec::new();
                    // check for vec![val; N] repeat syntax first
                    if !self.check(&close) {
                        let first = self.parse_expr(0)?;
                        if self.eat(&TokenKind::Semi) {
                            // Encode as __vec_repeat__ macro
                            let count = self.parse_expr(0)?;
                            // Consume closing bracket then return early with special macro
                            self.expect(&close)?;
                            return Ok(Expr::Macro {
                                name: format!("__vec_repeat__{}", name),
                                args: vec![first, count],
                            });
                        }
                        args.push(first);
                        while self.eat(&TokenKind::Comma) {
                            if self.check(&close) {
                                break;
                            }
                            args.push(self.parse_expr(0)?);
                        }
                    }
                    args
                };
                self.expect(&close)?;
                Ok(Expr::Macro { name, args })
            }

            // Parenthesized or tuple
            TokenKind::LParen => {
                self.advance();
                if self.eat(&TokenKind::RParen) {
                    return Ok(Expr::Tuple(vec![]));
                }
                let first = self.parse_expr(0)?;
                if self.eat(&TokenKind::Comma) {
                    let mut elems = vec![first];
                    while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                        elems.push(self.parse_expr(0)?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Expr::Tuple(elems))
                } else {
                    self.expect(&TokenKind::RParen)?;
                    Ok(first)
                }
            }

            // Array literal
            TokenKind::LBracket => {
                self.advance();
                if self.check(&TokenKind::RBracket) {
                    self.advance();
                    return Ok(Expr::Array(vec![]));
                }
                let first = self.parse_expr(0)?;
                // [expr; N] — array repeat syntax
                if self.eat(&TokenKind::Semi) {
                    let count_expr = self.parse_expr(0)?;
                    self.expect(&TokenKind::RBracket)?;
                    return Ok(Expr::Macro {
                        name: "__array_repeat__".into(),
                        args: vec![first, count_expr],
                    });
                }
                let mut elems = vec![first];
                if self.eat(&TokenKind::Comma) {
                    while !self.check(&TokenKind::RBracket) && !self.check(&TokenKind::Eof) {
                        elems.push(self.parse_expr(0)?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(Expr::Array(elems))
            }

            // Block expression
            TokenKind::LBrace => {
                let block = self.parse_block()?;
                Ok(Expr::Block(block))
            }

            // async { ... } and async move { ... } — execute synchronously at Level 0-3
            TokenKind::Async => {
                self.advance();
                self.eat(&TokenKind::Move);
                let block = self.parse_block()?;
                Ok(Expr::Block(block))
            }

            // unsafe { ... } — execute normally at Level 0-3; flagged at Level 4
            TokenKind::Unsafe => {
                self.advance();
                let block = self.parse_block()?;
                Ok(Expr::Unsafe(block))
            }

            // Control flow
            TokenKind::Return => {
                self.advance();
                let val = if !matches!(
                    self.peek(),
                    TokenKind::Semi | TokenKind::RBrace | TokenKind::Eof
                ) {
                    Some(Box::new(self.parse_expr(0)?))
                } else {
                    None
                };
                Ok(Expr::Return(val))
            }
            TokenKind::Break => {
                self.advance();
                // Check for label: `break 'outer` or `break 'outer value`
                let label = if matches!(self.peek(), TokenKind::Label(_)) {
                    if let TokenKind::Label(l) = self.advance().clone() {
                        Some(l)
                    } else {
                        None
                    }
                } else {
                    None
                };
                let val = if label.is_none()
                    && !matches!(
                        self.peek(),
                        TokenKind::Semi | TokenKind::RBrace | TokenKind::Comma | TokenKind::Eof
                    ) {
                    Some(Box::new(self.parse_expr(0)?))
                } else {
                    None
                };
                Ok(Expr::Break(label, val))
            }
            TokenKind::Continue => {
                self.advance();
                let label = if matches!(self.peek(), TokenKind::Label(_)) {
                    if let TokenKind::Label(l) = self.advance().clone() {
                        Some(l)
                    } else {
                        None
                    }
                } else {
                    None
                };
                Ok(Expr::Continue(label))
            }

            // if expression
            TokenKind::If => {
                self.advance();
                // `if let` — treat as always-true for Level 0, just run the body
                if self.check(&TokenKind::Let) {
                    self.advance();
                    let _pat = self.parse_pat()?;
                    self.expect(&TokenKind::Eq)?;
                    let scrutinee = self.parse_expr(0)?;
                    let then_block = self.parse_block()?;
                    let else_expr = if self.eat(&TokenKind::Else) {
                        if self.check(&TokenKind::If) {
                            Some(Box::new(self.parse_expr(0)?))
                        } else {
                            Some(Box::new(Expr::Block(self.parse_block()?)))
                        }
                    } else {
                        None
                    };
                    // Build match arms; add a wildcard arm for the else
                    // branch (or `_ => ()` when no explicit else) so the
                    // generated match is exhaustive — rustc rejects an
                    // `if let` -> match lowering without it (E0004).
                    let mut arms = vec![MatchArm {
                        pat: _pat,
                        guard: None,
                        body: Expr::Block(then_block),
                    }];
                    let else_body = match else_expr {
                        Some(e) => *e,
                        None => Expr::Block(Block {
                            stmts: vec![],
                            tail: None,
                        }),
                    };
                    arms.push(MatchArm {
                        pat: Pat::Wild,
                        guard: None,
                        body: else_body,
                    });
                    return Ok(Expr::Match {
                        scrutinee: Box::new(scrutinee),
                        arms,
                    });
                }
                let cond = Box::new(self.parse_expr(0)?);
                let then_block = self.parse_block()?;
                let else_block = if self.eat(&TokenKind::Else) {
                    if self.check(&TokenKind::If) {
                        Some(Box::new(self.parse_expr(0)?))
                    } else {
                        Some(Box::new(Expr::Block(self.parse_block()?)))
                    }
                } else {
                    None
                };
                Ok(Expr::If {
                    cond,
                    then_block,
                    else_block,
                })
            }

            // while
            TokenKind::While => {
                self.advance();
                // while let
                if self.check(&TokenKind::Let) {
                    self.advance();
                    let pat = self.parse_pat()?;
                    self.expect(&TokenKind::Eq)?;
                    let scrutinee = self.parse_expr(0)?;
                    let body = self.parse_block()?;
                    // Desugar while-let to loop { match scrutinee { pat => body, _ => break } }
                    let break_arm = MatchArm {
                        pat: Pat::Wild,
                        guard: None,
                        body: Expr::Break(None, None),
                    };
                    let match_arm = MatchArm {
                        pat,
                        guard: None,
                        body: Expr::Block(body),
                    };
                    let match_expr = Expr::Match {
                        scrutinee: Box::new(scrutinee),
                        arms: vec![match_arm, break_arm],
                    };
                    return Ok(Expr::Match {
                        scrutinee: Box::new(Expr::Lit(Lit::Bool(true))),
                        arms: vec![MatchArm {
                            pat: Pat::Ident("__loop__".into()),
                            guard: None,
                            body: match_expr,
                        }],
                    });
                }
                let cond = Box::new(self.parse_expr(0)?);
                let body_block = self.parse_block()?;
                // Desugar to loop { if !cond { break } body... }
                let if_break = Expr::If {
                    cond: Box::new(Expr::Unary(UnOp::Not, cond)),
                    then_block: Block {
                        stmts: vec![Stmt::Semi(Expr::Break(None, None))],
                        tail: None,
                    },
                    else_block: None,
                };
                let mut stmts = vec![Stmt::Expr(if_break)];
                stmts.extend(body_block.stmts);
                if let Some(tail) = body_block.tail {
                    stmts.push(Stmt::Expr(*tail));
                }
                // Wrap in the __loop__ sentinel so eval_loop_body handles the looping
                Ok(Expr::Match {
                    scrutinee: Box::new(Expr::Lit(Lit::Bool(true))),
                    arms: vec![MatchArm {
                        pat: Pat::Ident("__loop__".into()),
                        guard: None,
                        body: Expr::Block(Block { stmts, tail: None }),
                    }],
                })
            }

            // loop
            TokenKind::Loop => {
                self.advance();
                let body = self.parse_block()?;
                // loop is just a Block with a special marker — we'll handle in eval
                // Use a special sentinel: match true { true => body } in a loop
                // Actually, easier: keep it as a loop expression
                // We don't have a Loop AST node; desugar to a while-true
                // Return as Block with loop semantics via a sentinel match
                Ok(Expr::Match {
                    scrutinee: Box::new(Expr::Lit(Lit::Bool(true))),
                    arms: vec![MatchArm {
                        pat: Pat::Ident("__loop__".into()),
                        guard: None,
                        body: Expr::Block(body),
                    }],
                })
            }

            // labeled loop: 'outer: for / while / loop
            TokenKind::Label(label_name) => {
                let label_name = label_name.clone();
                self.advance();
                match self.peek().clone() {
                    TokenKind::For => {
                        self.advance();
                        let pat = self.parse_pat()?;
                        self.expect(&TokenKind::In)?;
                        let iter = Box::new(self.parse_expr(0)?);
                        let body = self.parse_block()?;
                        Ok(Expr::Macro {
                            name: format!("__for__:{}", label_name),
                            args: vec![
                                Expr::Block(Block {
                                    stmts: vec![Stmt::Expr(Expr::Ident(format!(
                                        "__pat__{}",
                                        pat_to_str(&pat)
                                    )))],
                                    tail: None,
                                }),
                                *iter,
                                Expr::Block(body),
                            ],
                        })
                    }
                    TokenKind::While => {
                        let sentinel = format!("__loop__:{}", label_name);
                        self.advance();
                        if self.check(&TokenKind::Let) {
                            self.advance();
                            let pat = self.parse_pat()?;
                            self.expect(&TokenKind::Eq)?;
                            let scrutinee = self.parse_expr(0)?;
                            let body = self.parse_block()?;
                            let break_arm = MatchArm {
                                pat: Pat::Wild,
                                guard: None,
                                body: Expr::Break(None, None),
                            };
                            let match_arm = MatchArm {
                                pat,
                                guard: None,
                                body: Expr::Block(body),
                            };
                            let match_expr = Expr::Match {
                                scrutinee: Box::new(scrutinee),
                                arms: vec![match_arm, break_arm],
                            };
                            return Ok(Expr::Match {
                                scrutinee: Box::new(Expr::Lit(Lit::Bool(true))),
                                arms: vec![MatchArm {
                                    pat: Pat::Ident(sentinel),
                                    guard: None,
                                    body: match_expr,
                                }],
                            });
                        }
                        let cond = Box::new(self.parse_expr(0)?);
                        let body_block = self.parse_block()?;
                        let if_break = Expr::If {
                            cond: Box::new(Expr::Unary(UnOp::Not, cond)),
                            then_block: Block {
                                stmts: vec![Stmt::Semi(Expr::Break(None, None))],
                                tail: None,
                            },
                            else_block: None,
                        };
                        let mut stmts = vec![Stmt::Expr(if_break)];
                        stmts.extend(body_block.stmts);
                        if let Some(tail) = body_block.tail {
                            stmts.push(Stmt::Expr(*tail));
                        }
                        Ok(Expr::Match {
                            scrutinee: Box::new(Expr::Lit(Lit::Bool(true))),
                            arms: vec![MatchArm {
                                pat: Pat::Ident(sentinel),
                                guard: None,
                                body: Expr::Block(Block { stmts, tail: None }),
                            }],
                        })
                    }
                    TokenKind::Loop => {
                        self.advance();
                        let body = self.parse_block()?;
                        Ok(Expr::Match {
                            scrutinee: Box::new(Expr::Lit(Lit::Bool(true))),
                            arms: vec![MatchArm {
                                pat: Pat::Ident(format!("__loop__:{}", label_name)),
                                guard: None,
                                body: Expr::Block(body),
                            }],
                        })
                    }
                    _ => Err(CrustError::parse(
                        format!(
                            "label `'{}` must precede a loop (`for`, `while`, or `loop`)",
                            label_name
                        ),
                        self.line(),
                    )),
                }
            }

            // for
            TokenKind::For => {
                self.advance();
                let pat = self.parse_pat()?;
                self.expect(&TokenKind::In)?;
                let iter = Box::new(self.parse_expr(0)?);
                let body = self.parse_block()?;
                // We'll encode for-in as a special Macro node at the expression level
                Ok(Expr::Macro {
                    name: "__for__".into(),
                    args: vec![
                        Expr::Block(Block {
                            stmts: vec![Stmt::Expr(Expr::Ident(format!(
                                "__pat__{}",
                                pat_to_str(&pat)
                            )))],
                            tail: None,
                        }),
                        *iter,
                        Expr::Block(body),
                    ],
                })
            }

            // match
            TokenKind::Match => {
                self.advance();
                let scrutinee = Box::new(self.parse_expr(0)?);
                self.expect(&TokenKind::LBrace)?;
                let mut arms = Vec::new();
                while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
                    let pat = self.parse_pat()?;
                    let guard = if self.eat(&TokenKind::If) {
                        Some(self.parse_expr(0)?)
                    } else {
                        None
                    };
                    self.expect(&TokenKind::FatArrow)?;
                    let body = if self.check(&TokenKind::LBrace) {
                        let b = self.parse_block()?;
                        self.eat(&TokenKind::Comma);
                        Expr::Block(b)
                    } else {
                        let e = self.parse_expr(0)?;
                        self.eat(&TokenKind::Comma);
                        e
                    };
                    arms.push(MatchArm { pat, guard, body });
                }
                self.expect(&TokenKind::RBrace)?;
                Ok(Expr::Match { scrutinee, arms })
            }

            // closure: |args| expr  or  |args| { block }  or  move |args| expr
            TokenKind::Or | TokenKind::OrOr | TokenKind::Move => {
                if self.check(&TokenKind::Move) {
                    self.advance();
                }
                let (params, _had_pipe) = if self.check(&TokenKind::OrOr) {
                    self.advance();
                    (vec![], true)
                } else {
                    self.advance(); // consume |
                    let mut ps = Vec::new();
                    while !self.check(&TokenKind::Or) && !self.check(&TokenKind::Eof) {
                        // strip leading refs: &, &&, mut
                        while self.eat(&TokenKind::And) || self.eat(&TokenKind::AndAnd) {}
                        self.eat(&TokenKind::Mut);
                        if self.check(&TokenKind::LParen) {
                            // Full pattern destructuring: |(k, v)| or |(i, (a, b))|
                            let pat = self.parse_pat_single()?;
                            if self.eat(&TokenKind::Colon) {
                                let _ = self.parse_ty()?;
                            }
                            // Check if it's a simple flat tuple (all idents) → use Tuple for compat
                            let cp = match &pat {
                                crate::ast::Pat::Tuple(parts)
                                    if parts
                                        .iter()
                                        .all(|p| matches!(p, crate::ast::Pat::Ident(_))) =>
                                {
                                    let names: Vec<String> = parts
                                        .iter()
                                        .map(|p| {
                                            if let crate::ast::Pat::Ident(n) = p {
                                                n.clone()
                                            } else {
                                                "_".into()
                                            }
                                        })
                                        .collect();
                                    crate::ast::ClosureParam::Tuple(names)
                                }
                                other => crate::ast::ClosureParam::Pat(other.clone()),
                            };
                            ps.push(cp);
                        } else {
                            let name = if self.check(&TokenKind::Underscore) {
                                self.advance();
                                "_".into()
                            } else {
                                self.expect_ident()?
                            };
                            if self.eat(&TokenKind::Colon) {
                                let _ = self.parse_ty()?;
                            }
                            ps.push(crate::ast::ClosureParam::Simple(name));
                        }
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    if !self.eat(&TokenKind::Or) {
                        return Err(CrustError::parse(
                            "expected | to close closure params",
                            self.line(),
                        ));
                    }
                    (ps, false)
                };
                // optional return type
                if self.eat(&TokenKind::Arrow) {
                    let _ = self.parse_ty()?;
                }
                let body = if self.check(&TokenKind::LBrace) {
                    Box::new(Expr::Block(self.parse_block()?))
                } else {
                    Box::new(self.parse_expr(0)?)
                };
                Ok(Expr::Closure { params, body })
            }

            // Identifier: could be variable, path, or struct literal
            TokenKind::Ident(name) => {
                self.advance();
                // build path: a::b::c
                let mut path = vec![name.clone()];
                while self.eat(&TokenKind::ColonColon) {
                    match self.peek().clone() {
                        TokenKind::Ident(s) => {
                            self.advance();
                            path.push(s);
                        }
                        TokenKind::Lt => {
                            self.skip_generics();
                        }
                        _ => break,
                    }
                }

                if path.len() > 1 {
                    // check for struct literal: Path { field: val, ... }
                    if self.check(&TokenKind::LBrace) && self.looks_like_struct_lit() {
                        let sname = path.join("::");
                        return self.parse_struct_lit(sname);
                    }
                    // function call on path
                    if self.check(&TokenKind::LParen) {
                        self.advance();
                        let args = self.parse_args()?;
                        self.expect(&TokenKind::RParen)?;
                        return Ok(Expr::Call {
                            func: Box::new(Expr::Path(path)),
                            args,
                        });
                    }
                    return Ok(Expr::Path(path));
                }

                // single ident: check for struct literal
                // Only consider it a struct literal if name starts with uppercase (Rust convention for types)
                let ident_is_type = name.chars().next().is_some_and(|c| c.is_uppercase());
                if self.check(&TokenKind::LBrace) && ident_is_type && self.looks_like_struct_lit() {
                    return self.parse_struct_lit(name);
                }

                Ok(Expr::Ident(name))
            }

            // Self
            TokenKind::SelfKw => {
                self.advance();
                Ok(Expr::Ident("self".to_string()))
            }

            // ..expr (range from start)
            TokenKind::DotDot => {
                self.advance();
                let end = if !matches!(
                    self.peek(),
                    TokenKind::Semi
                        | TokenKind::RBracket
                        | TokenKind::RParen
                        | TokenKind::RBrace
                        | TokenKind::Comma
                        | TokenKind::Eof
                ) {
                    Some(Box::new(self.parse_expr(2)?))
                } else {
                    None
                };
                Ok(Expr::Range {
                    start: None,
                    end,
                    inclusive: false,
                })
            }
            TokenKind::DotDotEq => {
                self.advance();
                let end = Box::new(self.parse_expr(2)?);
                Ok(Expr::Range {
                    start: None,
                    end: Some(end),
                    inclusive: true,
                })
            }

            other => Err(CrustError::parse(
                format!("unexpected token in expression: {:?}", other),
                self.line(),
            )),
        }
    }

    /// Heuristic: are we looking at `{ field: expr, ... }` (struct lit) vs a block?
    fn looks_like_struct_lit(&self) -> bool {
        let mut i = self.pos + 1;
        let len = self.tokens.len();
        if i >= len {
            return false;
        }
        match &self.tokens[i].kind {
            TokenKind::Ident(_) => {
                i += 1;
                if i >= len {
                    return false;
                }
                match &self.tokens[i].kind {
                    // `{ field: expr }` — definitely a struct literal
                    TokenKind::Colon => true,
                    // `{ field }` — single shorthand field, treat as struct literal
                    TokenKind::RBrace => true,
                    // `{ field, ... }` — struct shorthand only if next field also ends in `:` or `,`
                    TokenKind::Comma => {
                        // Scan ahead: if any field has `:` it's a struct lit
                        // Also, if every non-comma token is an Ident (all shorthand), treat as struct lit
                        let mut j = i + 1;
                        let mut all_ident = true;
                        while j < len {
                            match &self.tokens[j].kind {
                                TokenKind::Colon => return true,
                                TokenKind::RBrace => return all_ident,
                                TokenKind::Eof => return false,
                                TokenKind::Ident(_) | TokenKind::Comma => {
                                    j += 1;
                                }
                                _ => {
                                    all_ident = false;
                                    j += 1;
                                }
                            }
                        }
                        false
                    }
                    _ => false,
                }
            }
            TokenKind::DotDot => true,
            _ => false,
        }
    }

    fn parse_struct_lit(&mut self, name: String) -> Result<Expr> {
        self.expect(&TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            if self.eat(&TokenKind::DotDot) {
                // struct update syntax: ..other — encode as __rest__ field
                let base = self.parse_expr(0)?;
                fields.push(("__rest__".to_string(), base));
                break;
            }
            let fname = self.expect_ident()?;
            if self.eat(&TokenKind::Colon) {
                let val = self.parse_expr(0)?;
                fields.push((fname, val));
            } else {
                // shorthand: `Point { x }` means `Point { x: x }`
                fields.push((fname.clone(), Expr::Ident(fname)));
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Expr::StructLit { name, fields })
    }

    // ── Patterns ─────────────────────────────────────────────────────────────

    fn parse_pat(&mut self) -> Result<Pat> {
        let first = self.parse_pat_single()?;
        if self.check(&TokenKind::Or) {
            let mut pats = vec![first];
            while self.eat(&TokenKind::Or) {
                pats.push(self.parse_pat_single()?);
            }
            Ok(Pat::Or(pats))
        } else {
            Ok(first)
        }
    }

    fn parse_pat_single(&mut self) -> Result<Pat> {
        self.eat(&TokenKind::Ref);
        self.eat(&TokenKind::Mut);
        match self.peek().clone() {
            TokenKind::Underscore => {
                self.advance();
                Ok(Pat::Wild)
            }
            TokenKind::DotDot => {
                self.advance();
                Ok(Pat::Wild)
            }
            TokenKind::Int(n) => {
                self.advance();
                self.maybe_range_pat(Lit::Int(n))
            }
            TokenKind::Float(f) => {
                self.advance();
                Ok(Pat::Lit(Lit::Float(f)))
            }
            TokenKind::True => {
                self.advance();
                Ok(Pat::Lit(Lit::Bool(true)))
            }
            TokenKind::False => {
                self.advance();
                Ok(Pat::Lit(Lit::Bool(false)))
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Pat::Lit(Lit::Str(s)))
            }
            TokenKind::Char(c) => {
                self.advance();
                self.maybe_range_pat(Lit::Char(c))
            }
            TokenKind::Minus => {
                self.advance();
                match self.peek().clone() {
                    TokenKind::Int(n) => {
                        self.advance();
                        self.maybe_range_pat(Lit::Int(-n))
                    }
                    TokenKind::Float(f) => {
                        self.advance();
                        Ok(Pat::Lit(Lit::Float(-f)))
                    }
                    _ => Err(CrustError::parse(
                        "expected literal after - in pattern",
                        self.line(),
                    )),
                }
            }
            TokenKind::And | TokenKind::AndAnd => {
                self.advance(); // consume & or &&
                self.eat(&TokenKind::Mut);
                let inner = self.parse_pat_single()?;
                Ok(Pat::Ref(Box::new(inner)))
            }
            TokenKind::LParen => {
                self.advance();
                let mut pats = Vec::new();
                while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                    pats.push(self.parse_pat()?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen)?;
                Ok(Pat::Tuple(pats))
            }
            TokenKind::SelfKw => {
                self.advance();
                Ok(Pat::Ident("self".to_string()))
            }
            TokenKind::Ident(name) => {
                let name = {
                    self.advance();
                    name
                };
                // @ binding: name @ pattern
                if self.eat(&TokenKind::At) {
                    let sub = self.parse_pat_single()?;
                    return Ok(Pat::Bind {
                        name,
                        pat: Box::new(sub),
                    });
                }
                // path: Some(x), None, MyEnum::Variant
                let mut path = vec![name.clone()];
                while self.eat(&TokenKind::ColonColon) {
                    match self.peek().clone() {
                        TokenKind::Ident(s) => {
                            self.advance();
                            path.push(s);
                        }
                        _ => break,
                    }
                }
                let full_name = path.join("::");

                if self.check(&TokenKind::LBrace) {
                    // struct pattern
                    self.advance();
                    let mut fields = Vec::new();
                    let mut rest = false;
                    while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
                        if self.eat(&TokenKind::DotDot) {
                            rest = true;
                            break;
                        }
                        let fname = self.expect_ident()?;
                        let fpat = if self.eat(&TokenKind::Colon) {
                            self.parse_pat()?
                        } else {
                            Pat::Ident(fname.clone())
                        };
                        fields.push((fname, fpat));
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RBrace)?;
                    Ok(Pat::Struct {
                        name: full_name,
                        fields,
                        rest,
                    })
                } else if self.check(&TokenKind::LParen) {
                    // tuple struct pattern: Some(x), Ok(v), Err(e)
                    self.advance();
                    let mut pats = Vec::new();
                    while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                        if self.eat(&TokenKind::DotDot) {
                            break;
                        }
                        pats.push(self.parse_pat()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Pat::TupleStruct {
                        name: full_name,
                        fields: pats,
                    })
                } else if self.eat(&TokenKind::DotDotEq) {
                    // path range: i64::MIN..=-1, 'a'..='z'
                    let start_lit = path_to_lit(&full_name).ok_or_else(|| {
                        CrustError::parse(
                            format!("unknown constant in range: {}", full_name),
                            self.line(),
                        )
                    })?;
                    let end_lit = self.parse_range_end_lit()?;
                    Ok(Pat::Range(start_lit, end_lit, true))
                } else {
                    Ok(Pat::Ident(full_name))
                }
            }
            TokenKind::LBracket => {
                self.advance();
                let mut before: Vec<Pat> = Vec::new();
                let mut rest: Option<String> = None;
                let mut has_rest = false;
                let mut after: Vec<Pat> = Vec::new();
                while !self.check(&TokenKind::RBracket) && !self.check(&TokenKind::Eof) {
                    // Check for `name @ ..` or bare `..`
                    let is_dotdot = self.check(&TokenKind::DotDot);
                    // Check for `name @ ..`: peek ahead — but we can detect it by seeing
                    // if the current token is an Ident followed by @ followed by ..
                    // We'll handle bare `..` first, then `name @ ..` is caught by normal
                    // ident parsing which sees `@` and tries parse_pat_single which gives Wild for `..`
                    if is_dotdot {
                        self.advance(); // consume ..
                        has_rest = true;
                        if self.eat(&TokenKind::Comma) {
                            continue;
                        }
                        break;
                    }
                    // Parse a regular pattern; if it turns out to be `name @ ..` the
                    // `@` branch will call parse_pat_single which returns Pat::Wild for `..`.
                    // We intercept that here by checking before parsing.
                    // Actually: check for ident @ .. specially
                    if let TokenKind::Ident(ref name) = self.peek().clone() {
                        // peek one further
                        let name = name.clone();
                        let pos = self.pos;
                        self.advance(); // consume ident
                        if self.eat(&TokenKind::At) {
                            if self.check(&TokenKind::DotDot) {
                                self.advance(); // consume ..
                                has_rest = true;
                                rest = Some(name);
                                if self.eat(&TokenKind::Comma) {
                                    continue;
                                }
                                break;
                            }
                            // Not `..` after @, restore and re-parse normally
                            // (We already consumed ident and @, so parse sub-pattern)
                            let sub = self.parse_pat_single()?;
                            let pat = Pat::Bind {
                                name,
                                pat: Box::new(sub),
                            };
                            if !has_rest {
                                before.push(pat);
                            } else {
                                after.push(pat);
                            }
                        } else {
                            // Not an @ binding — check path/struct/etc by rewinding
                            self.pos = pos;
                            let pat = self.parse_pat()?;
                            if !has_rest {
                                before.push(pat);
                            } else {
                                after.push(pat);
                            }
                        }
                    } else {
                        let pat = self.parse_pat()?;
                        if !has_rest {
                            before.push(pat);
                        } else {
                            after.push(pat);
                        }
                    }
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(Pat::Slice {
                    before,
                    rest,
                    has_rest,
                    after,
                })
            }
            other => Err(CrustError::parse(
                format!("expected pattern, got {:?}", other),
                self.line(),
            )),
        }
    }

    fn parse_range_end_lit(&mut self) -> Result<Lit> {
        match self.peek().clone() {
            TokenKind::Int(n) => {
                self.advance();
                Ok(Lit::Int(n))
            }
            TokenKind::Char(c) => {
                self.advance();
                Ok(Lit::Char(c))
            }
            TokenKind::Minus => {
                self.advance();
                match self.peek().clone() {
                    TokenKind::Int(n) => {
                        self.advance();
                        Ok(Lit::Int(-n))
                    }
                    _ => Err(CrustError::parse(
                        "expected literal in range pattern",
                        self.line(),
                    )),
                }
            }
            TokenKind::Ident(name) => {
                self.advance();
                let mut path = vec![name];
                while self.eat(&TokenKind::ColonColon) {
                    if let TokenKind::Ident(s) = self.peek().clone() {
                        self.advance();
                        path.push(s);
                    } else {
                        break;
                    }
                }
                let full = path.join("::");
                path_to_lit(&full).ok_or_else(|| {
                    CrustError::parse(format!("unknown constant in range: {}", full), self.line())
                })
            }
            _ => Err(CrustError::parse(
                "expected literal in range pattern",
                self.line(),
            )),
        }
    }

    fn maybe_range_pat(&mut self, start: Lit) -> Result<Pat> {
        if !self.eat(&TokenKind::DotDotEq) {
            return Ok(Pat::Lit(start));
        }
        let end = self.parse_range_end_lit()?;
        Ok(Pat::Range(start, end, true))
    }

    // Parse macro args: format strings get first arg as raw string, rest as exprs
    fn parse_macro_args(&mut self) -> Result<Vec<Expr>> {
        let mut args = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
            args.push(self.parse_expr(0)?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        Ok(args)
    }
}

fn path_to_lit(path: &str) -> Option<Lit> {
    match path {
        "i8::MIN" | "std::i8::MIN" => Some(Lit::Int(i8::MIN as i64)),
        "i8::MAX" | "std::i8::MAX" => Some(Lit::Int(i8::MAX as i64)),
        "i16::MIN" | "std::i16::MIN" => Some(Lit::Int(i16::MIN as i64)),
        "i16::MAX" | "std::i16::MAX" => Some(Lit::Int(i16::MAX as i64)),
        "i32::MIN" | "std::i32::MIN" => Some(Lit::Int(i32::MIN as i64)),
        "i32::MAX" | "std::i32::MAX" => Some(Lit::Int(i32::MAX as i64)),
        "i64::MIN" | "std::i64::MIN" => Some(Lit::Int(i64::MIN)),
        "i64::MAX" | "std::i64::MAX" => Some(Lit::Int(i64::MAX)),
        "u8::MIN" | "std::u8::MIN" => Some(Lit::Int(u8::MIN as i64)),
        "u8::MAX" | "std::u8::MAX" => Some(Lit::Int(u8::MAX as i64)),
        "u16::MIN" => Some(Lit::Int(u16::MIN as i64)),
        "u16::MAX" => Some(Lit::Int(u16::MAX as i64)),
        "u32::MIN" => Some(Lit::Int(u32::MIN as i64)),
        "u32::MAX" => Some(Lit::Int(u32::MAX as i64)),
        "usize::MAX" | "u64::MAX" => Some(Lit::Int(i64::MAX)),
        _ => None,
    }
}

/// Parse a single `Attr` token's content string into a structured `crate::ast::Attr`.
///
/// Well-known crust attributes:
/// - `pure`                — marks a function as free of side effects
/// - `requires(pred)`      — precondition expression
/// - `ensures(pred)`       — postcondition expression
/// - `invariant(pred)`     — loop / function invariant
///
/// All other attribute strings are stored as `Attr::Unknown` so the code
/// generator can re-emit them verbatim (e.g. `derive(Clone, Debug)`).
fn parse_attr_content(content: &str) -> crate::ast::Attr {
    use crate::ast::Attr;
    let s = content.trim();
    if s == "pure" {
        return Attr::Pure;
    }
    if let Some(inner) = s
        .strip_prefix("requires(")
        .and_then(|t| t.strip_suffix(')'))
    {
        match parse_expr_str(inner) {
            Ok(expr) => return Attr::Requires(expr),
            Err(e) => eprintln!(
                "warning: failed to parse #[requires({})] predicate: {}",
                inner, e
            ),
        }
    }
    if let Some(inner) = s.strip_prefix("ensures(").and_then(|t| t.strip_suffix(')')) {
        match parse_expr_str(inner) {
            Ok(expr) => return Attr::Ensures(expr),
            Err(e) => eprintln!(
                "warning: failed to parse #[ensures({})] predicate: {}",
                inner, e
            ),
        }
    }
    if let Some(inner) = s
        .strip_prefix("invariant(")
        .and_then(|t| t.strip_suffix(')'))
    {
        match parse_expr_str(inner) {
            Ok(expr) => return Attr::Invariant(expr),
            Err(e) => eprintln!(
                "warning: failed to parse #[invariant({})] predicate: {}",
                inner, e
            ),
        }
    }
    Attr::Unknown(content.to_string())
}

/// Re-tokenize and parse a short expression string (used for contract predicates).
fn parse_expr_str(src: &str) -> crate::error::Result<crate::ast::Expr> {
    let tokens = crate::lexer::Lexer::new(src).tokenize()?;
    Parser::new(tokens).parse_expr(0)
}

fn is_block_stmt_expr(e: &crate::ast::Expr) -> bool {
    match e {
        crate::ast::Expr::Macro { name, .. } => {
            matches!(name.as_str(), "__for__" | "__while__")
        }
        _ => false,
    }
}

fn pat_to_str(pat: &Pat) -> String {
    match pat {
        Pat::Ident(s) => s.clone(),
        Pat::Wild => "_".into(),
        Pat::Ref(inner) => pat_to_str(inner), // &x or &&x → x
        Pat::Tuple(ps) => format!(
            "({})",
            ps.iter().map(pat_to_str).collect::<Vec<_>>().join(",")
        ),
        Pat::TupleStruct { fields, .. } => {
            format!(
                "({})",
                fields.iter().map(pat_to_str).collect::<Vec<_>>().join(",")
            )
        }
        _ => "_".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> Program {
        let tokens = Lexer::new(src).tokenize().unwrap();
        Parser::new(tokens).parse_program().unwrap()
    }

    fn read_example(name: &str) -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples")
                .join(name),
        )
        .unwrap()
    }

    #[test]
    fn parses_fib() {
        let src = read_example("fib.crust");
        let prog = parse(&src);
        assert_eq!(prog.len(), 2); // fib + main
    }

    #[test]
    fn parses_hello() {
        let src = read_example("hello.crust");
        let prog = parse(&src);
        assert_eq!(prog.len(), 2); // greet + main
    }

    #[test]
    fn parses_point() {
        let src = read_example("point.crust");
        let prog = parse(&src);
        assert_eq!(prog.len(), 3); // Point struct + impl + main
    }

    #[test]
    fn parses_all_examples() {
        let examples_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        for entry in std::fs::read_dir(examples_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("crust") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            parse(&src);
        }
    }
}
