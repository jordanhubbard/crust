use crate::ast::*;
/// Recursive-descent parser for Crust.
use crate::lexer::{SpannedToken, Token};

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn peek_at(&self, offset: usize) -> &Token {
        let i = self.pos + offset;
        if i < self.tokens.len() {
            &self.tokens[i].token
        } else {
            &Token::Eof
        }
    }

    fn line(&self) -> usize {
        self.tokens[self.pos].line
    }

    fn advance(&mut self) {
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, tok: &Token) -> Result<(), String> {
        if self.peek() == tok {
            self.advance();
            Ok(())
        } else {
            Err(format!(
                "line {}: expected {:?}, found {:?}",
                self.line(),
                tok,
                self.peek()
            ))
        }
    }

    fn eat(&mut self, tok: &Token) -> bool {
        if self.peek() == tok {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        if let Token::Ident(name) = self.peek().clone() {
            self.advance();
            Ok(name)
        } else {
            Err(format!(
                "line {}: expected identifier, found {:?}",
                self.line(),
                self.peek()
            ))
        }
    }

    /// Consume `mut` keyword if present (it's lexed as Token::Mut).
    fn eat_mut(&mut self) -> bool {
        self.eat(&Token::Mut)
    }

    /// Skip a type annotation — consumed but discarded at Level 0.
    fn skip_type(&mut self) -> Result<(), String> {
        // Handle & and &mut prefixes
        if self.eat(&Token::Amp) {
            self.eat_mut();
        }
        match self.peek().clone() {
            Token::Ident(_) => {
                self.advance();
                // Handle Vec<T>, Option<T>, Result<T,E>, etc.
                if self.peek() == &Token::Lt {
                    self.advance();
                    let mut depth = 1usize;
                    while depth > 0 && self.peek() != &Token::Eof {
                        match self.peek() {
                            Token::Lt => {
                                depth += 1;
                                self.advance();
                            }
                            Token::Gt => {
                                depth -= 1;
                                self.advance();
                            }
                            _ => {
                                self.advance();
                            }
                        }
                    }
                }
                Ok(())
            }
            Token::LParen => {
                // () unit type or tuple
                self.advance();
                self.expect(&Token::RParen)?;
                Ok(())
            }
            _ => Ok(()), // best-effort; don't fail on exotic types
        }
    }

    // ----------------------------------------------------------------
    // Top-level
    // ----------------------------------------------------------------

    pub fn parse_program(&mut self) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        while self.peek() != &Token::Eof {
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(&Token::LBrace)?;
        let mut stmts = Vec::new();
        while self.peek() != &Token::RBrace && self.peek() != &Token::Eof {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        // Strip leading `pub` — Level 0 ignores visibility
        self.eat(&Token::Pub);

        match self.peek().clone() {
            Token::Let => self.parse_let(),
            Token::Fn => self.parse_fn_def(),
            Token::Struct => self.parse_struct_def(),
            Token::While => self.parse_while(),
            Token::Loop => self.parse_loop_stmt(),
            Token::For => self.parse_for(),
            Token::Impl => self.skip_impl(),
            Token::Use => self.skip_use(),
            Token::Break => {
                self.advance();
                self.eat(&Token::Semicolon);
                Ok(Stmt::Break)
            }
            Token::Return => self.parse_return(),
            _ => self.parse_assign_or_expr_stmt(),
        }
    }

    // ----------------------------------------------------------------
    // Statements
    // ----------------------------------------------------------------

    fn parse_let(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Let)?;
        self.eat_mut();
        let name = self.expect_ident()?;
        if self.eat(&Token::Colon) {
            self.skip_type()?;
        }
        self.expect(&Token::Eq)?;
        let value = self.parse_expr()?;
        self.eat(&Token::Semicolon);
        Ok(Stmt::Let { name, value })
    }

    fn parse_fn_def(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Fn)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        while self.peek() != &Token::RParen && self.peek() != &Token::Eof {
            self.eat_mut();
            // `self` parameter
            if let Token::Ident(s) = self.peek().clone() {
                if s == "self" {
                    self.advance();
                    self.eat(&Token::Comma);
                    continue;
                }
            }
            if self.eat(&Token::Amp) {
                self.eat_mut();
            }
            let pname = self.expect_ident()?;
            let ptype = if self.eat(&Token::Colon) {
                self.eat(&Token::Amp);
                self.eat_mut();
                if let Token::Ident(t) = self.peek().clone() {
                    self.advance();
                    t
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            params.push((pname, ptype));
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RParen)?;
        if self.eat(&Token::Arrow) {
            self.skip_type()?;
        }
        let body = self.parse_block()?;
        Ok(Stmt::FnDef { name, params, body })
    }

    fn parse_struct_def(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Struct)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LBrace)?;
        let mut fields = Vec::new();
        while self.peek() != &Token::RBrace && self.peek() != &Token::Eof {
            self.eat(&Token::Pub);
            let fname = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let ftype = if let Token::Ident(t) = self.peek().clone() {
                self.advance();
                t
            } else {
                String::new()
            };
            fields.push((fname, ftype));
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Stmt::StructDef { name, fields })
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::While)?;
        let condition = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While { condition, body })
    }

    fn parse_loop_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Loop)?;
        let body = self.parse_block()?;
        Ok(Stmt::Loop { body })
    }

    fn parse_for(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::For)?;
        let var = self.expect_ident()?;
        self.expect(&Token::In)?;
        // Parse up to additive level so `..` isn't consumed as part of sub-expression
        let start = self.parse_additive()?;
        let iter = if self.eat(&Token::DotDotEq) {
            let end = self.parse_additive()?;
            Expr::Range {
                start: Box::new(start),
                end: Box::new(end),
                inclusive: true,
            }
        } else if self.eat(&Token::DotDot) {
            let end = self.parse_additive()?;
            Expr::Range {
                start: Box::new(start),
                end: Box::new(end),
                inclusive: false,
            }
        } else {
            start
        };
        let body = self.parse_block()?;
        Ok(Stmt::For { var, iter, body })
    }

    fn parse_return(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Return)?;
        if self.peek() == &Token::Semicolon || self.peek() == &Token::RBrace {
            self.eat(&Token::Semicolon);
            return Ok(Stmt::Return(None));
        }
        let expr = self.parse_expr()?;
        self.eat(&Token::Semicolon);
        Ok(Stmt::Return(Some(expr)))
    }

    fn skip_use(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Use)?;
        while self.peek() != &Token::Semicolon && self.peek() != &Token::Eof {
            self.advance();
        }
        self.eat(&Token::Semicolon);
        Ok(Stmt::ExprStmt(Expr::BoolLit(false)))
    }

    fn skip_impl(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Impl)?;
        // skip to opening brace
        while self.peek() != &Token::LBrace && self.peek() != &Token::Eof {
            self.advance();
        }
        if self.peek() == &Token::LBrace {
            self.skip_balanced_braces()?;
        }
        Ok(Stmt::ExprStmt(Expr::BoolLit(false)))
    }

    fn skip_balanced_braces(&mut self) -> Result<(), String> {
        self.expect(&Token::LBrace)?;
        let mut depth = 1usize;
        while depth > 0 && self.peek() != &Token::Eof {
            match self.peek() {
                Token::LBrace => {
                    depth += 1;
                    self.advance();
                }
                Token::RBrace => {
                    depth -= 1;
                    self.advance();
                }
                _ => {
                    self.advance();
                }
            }
        }
        Ok(())
    }

    fn parse_assign_or_expr_stmt(&mut self) -> Result<Stmt, String> {
        let expr = self.parse_expr()?;

        // Compound assignment operators
        let op = match self.peek() {
            Token::PlusEq => Some(BinOp::Add),
            Token::MinusEq => Some(BinOp::Sub),
            Token::StarEq => Some(BinOp::Mul),
            Token::SlashEq => Some(BinOp::Div),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let rhs = self.parse_expr()?;
            self.eat(&Token::Semicolon);
            let target = expr_to_assign_target(expr)?;
            let lhs_expr = assign_target_to_expr(&target);
            let value = Expr::BinaryOp {
                left: Box::new(lhs_expr),
                op,
                right: Box::new(rhs),
            };
            return Ok(Stmt::Assign { target, value });
        }

        if self.eat(&Token::Eq) {
            let value = self.parse_expr()?;
            self.eat(&Token::Semicolon);
            let target = expr_to_assign_target(expr)?;
            return Ok(Stmt::Assign { target, value });
        }

        self.eat(&Token::Semicolon);
        Ok(Stmt::ExprStmt(expr))
    }

    // ----------------------------------------------------------------
    // Expressions (Pratt-style precedence climbing)
    // ----------------------------------------------------------------

    pub fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_and()?;
        while self.peek() == &Token::PipePipe {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::Or,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_comparison()?;
        while self.peek() == &Token::AmpAmp {
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let left = self.parse_additive()?;
        let op = match self.peek() {
            Token::EqEq => Some(BinOp::Eq),
            Token::BangEq => Some(BinOp::NotEq),
            Token::Lt => Some(BinOp::Lt),
            Token::Gt => Some(BinOp::Gt),
            Token::LtEq => Some(BinOp::LtEq),
            Token::GtEq => Some(BinOp::GtEq),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let right = self.parse_additive()?;
            return Ok(Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            });
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Token::Minus => {
                self.advance();
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    expr: Box::new(self.parse_unary()?),
                })
            }
            Token::Bang => {
                self.advance();
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    expr: Box::new(self.parse_unary()?),
                })
            }
            // Ignore reference operators at Level 0
            Token::Amp => {
                self.advance();
                self.eat_mut();
                self.parse_unary()
            }
            Token::Star => {
                self.advance();
                self.parse_unary()
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.eat(&Token::Dot) {
                let member = self.expect_ident()?;
                if self.eat(&Token::LParen) {
                    let mut args = Vec::new();
                    while self.peek() != &Token::RParen && self.peek() != &Token::Eof {
                        args.push(self.parse_expr()?);
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(&Token::RParen)?;
                    expr = Expr::MethodCall {
                        object: Box::new(expr),
                        method: member,
                        args,
                    };
                } else {
                    expr = Expr::FieldAccess {
                        object: Box::new(expr),
                        field: member,
                    };
                }
            } else if self.eat(&Token::LBracket) {
                let index = self.parse_expr()?;
                self.expect(&Token::RBracket)?;
                expr = Expr::IndexAccess {
                    object: Box::new(expr),
                    index: Box::new(index),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Token::IntLit(n) => {
                self.advance();
                Ok(Expr::IntLit(n))
            }
            Token::FloatLit(f) => {
                self.advance();
                Ok(Expr::FloatLit(f))
            }
            Token::BoolLit(b) => {
                self.advance();
                Ok(Expr::BoolLit(b))
            }
            Token::StringLit(s) => {
                self.advance();
                Ok(Expr::StringLit(s))
            }

            Token::LParen => {
                self.advance();
                if self.eat(&Token::RParen) {
                    return Ok(Expr::BoolLit(false));
                } // unit ()
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }

            Token::LBrace => {
                let stmts = self.parse_block()?;
                Ok(Expr::Block(stmts))
            }

            Token::If => {
                self.advance();
                let condition = self.parse_expr()?;
                let then_block = self.parse_block()?;
                let else_block = if self.eat(&Token::Else) {
                    if self.peek() == &Token::If {
                        Some(vec![self.parse_stmt()?])
                    } else {
                        Some(self.parse_block()?)
                    }
                } else {
                    None
                };
                Ok(Expr::If {
                    condition: Box::new(condition),
                    then_block,
                    else_block,
                })
            }

            Token::Ident(name) => {
                self.advance();
                // Macro call: name!
                if self.eat(&Token::Bang) {
                    return self.parse_macro(&name);
                }
                // Struct init: Uppercase + {  with  ident: pattern
                let first_upper = name.chars().next().map_or(false, |c| c.is_uppercase());
                if first_upper && self.peek() == &Token::LBrace {
                    if self.looks_like_struct_init() {
                        return self.parse_struct_init(name);
                    }
                }
                // Function call
                if self.eat(&Token::LParen) {
                    let mut args = Vec::new();
                    while self.peek() != &Token::RParen && self.peek() != &Token::Eof {
                        args.push(self.parse_expr()?);
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(&Token::RParen)?;
                    return Ok(Expr::Call {
                        function: Box::new(Expr::Ident(name)),
                        args,
                    });
                }
                Ok(Expr::Ident(name))
            }

            tok => Err(format!("line {}: unexpected token {:?}", self.line(), tok)),
        }
    }

    /// Heuristic: after `{`, does it look like `ident:` (struct field)?
    fn looks_like_struct_init(&self) -> bool {
        // tokens[pos] == LBrace, check tokens[pos+1] and tokens[pos+2]
        matches!(
            (self.peek_at(1), self.peek_at(2)),
            (Token::Ident(_), Token::Colon) | (Token::RBrace, _)
        )
    }

    fn parse_struct_init(&mut self, name: String) -> Result<Expr, String> {
        self.expect(&Token::LBrace)?;
        let mut fields = Vec::new();
        while self.peek() != &Token::RBrace && self.peek() != &Token::Eof {
            let fname = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let fval = self.parse_expr()?;
            fields.push((fname, fval));
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::StructInit { name, fields })
    }

    fn parse_macro(&mut self, name: &str) -> Result<Expr, String> {
        let (open, close) = if self.peek() == &Token::LBracket {
            (Token::LBracket, Token::RBracket)
        } else {
            (Token::LParen, Token::RParen)
        };
        self.expect(&open)?;

        match name {
            "println" | "print" => {
                let fmt_str = if let Token::StringLit(s) = self.peek().clone() {
                    self.advance();
                    s
                } else {
                    return Err(format!(
                        "line {}: {}! expects a string literal",
                        self.line(),
                        name
                    ));
                };
                let mut args = Vec::new();
                while self.eat(&Token::Comma) {
                    if self.peek() == &close {
                        break;
                    }
                    args.push(self.parse_expr()?);
                }
                self.expect(&close)?;
                Ok(Expr::PrintLn {
                    format_str: fmt_str,
                    args,
                    newline: name == "println",
                })
            }
            "vec" => {
                let mut elems = Vec::new();
                while self.peek() != &close && self.peek() != &Token::Eof {
                    elems.push(self.parse_expr()?);
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&close)?;
                Ok(Expr::VecLit(elems))
            }
            "panic" | "todo" | "unimplemented" => {
                let mut args = Vec::new();
                while self.peek() != &close && self.peek() != &Token::Eof {
                    args.push(self.parse_expr()?);
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&close)?;
                Ok(Expr::Call {
                    function: Box::new(Expr::Ident(format!("__builtin_{}", name))),
                    args,
                })
            }
            "format" => {
                // format!("...", args) → treat like println! but return String
                let fmt_str = if let Token::StringLit(s) = self.peek().clone() {
                    self.advance();
                    s
                } else {
                    return Err(format!(
                        "line {}: format! expects a string literal",
                        self.line()
                    ));
                };
                let mut args = Vec::new();
                while self.eat(&Token::Comma) {
                    if self.peek() == &close {
                        break;
                    }
                    args.push(self.parse_expr()?);
                }
                self.expect(&close)?;
                Ok(Expr::Call {
                    function: Box::new(Expr::Ident("__builtin_format".to_string())),
                    args: std::iter::once(Expr::StringLit(fmt_str))
                        .chain(args)
                        .collect(),
                })
            }
            _ => {
                // Unknown macro: parse args and call as function
                let mut args = Vec::new();
                while self.peek() != &close && self.peek() != &Token::Eof {
                    args.push(self.parse_expr()?);
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&close)?;
                Ok(Expr::Call {
                    function: Box::new(Expr::Ident(format!("{}!", name))),
                    args,
                })
            }
        }
    }
}

fn expr_to_assign_target(expr: Expr) -> Result<AssignTarget, String> {
    match expr {
        Expr::Ident(name) => Ok(AssignTarget::Variable(name)),
        Expr::FieldAccess { object, field } => Ok(AssignTarget::Field(object, field)),
        Expr::IndexAccess { object, index } => Ok(AssignTarget::Index(object, index)),
        e => Err(format!("invalid assignment target: {:?}", e)),
    }
}

fn assign_target_to_expr(target: &AssignTarget) -> Expr {
    match target {
        AssignTarget::Variable(name) => Expr::Ident(name.clone()),
        AssignTarget::Field(obj, field) => Expr::FieldAccess {
            object: obj.clone(),
            field: field.clone(),
        },
        AssignTarget::Index(obj, idx) => Expr::IndexAccess {
            object: obj.clone(),
            index: idx.clone(),
        },
    }
}
