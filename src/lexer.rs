/// Lexer/tokenizer for Crust.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    BoolLit(bool),

    // Identifier
    Ident(String),

    // Keywords
    Let,
    Fn,
    Struct,
    If,
    Else,
    While,
    Loop,
    Break,
    Return,

    // Punctuation
    LParen,    // (
    RParen,    // )
    LBrace,    // {
    RBrace,    // }
    LBracket,  // [
    RBracket,  // ]
    Comma,     // ,
    Semicolon, // ;
    Colon,     // :
    Dot,       // .
    Arrow,     // ->

    // Operators
    Plus,      // +
    Minus,     // -
    Star,      // *
    Slash,     // /
    Percent,   // %
    Eq,        // =
    EqEq,      // ==
    BangEq,    // !=
    Lt,        // <
    Gt,        // >
    LtEq,      // <=
    GtEq,      // >=
    AmpAmp,    // &&
    PipePipe,  // ||
    Bang,      // !

    // Special
    Eof,
}

#[derive(Debug, Clone)]
pub struct SpannedToken {
    pub token: Token,
    pub line: usize,
    pub col: usize,
}

pub struct Lexer {
    source: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Lexer {
            source: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<SpannedToken>, String> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.pos >= self.source.len() {
                tokens.push(SpannedToken {
                    token: Token::Eof,
                    line: self.line,
                    col: self.col,
                });
                break;
            }
            let tok = self.next_token()?;
            tokens.push(tok);
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.pos).copied()
    }

    fn peek_ahead(&self, offset: usize) -> Option<char> {
        self.source.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> char {
        let ch = self.source[self.pos];
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        ch
    }

    fn skip_whitespace_and_comments(&mut self) {
        while self.pos < self.source.len() {
            let ch = self.source[self.pos];
            if ch.is_whitespace() {
                self.advance();
            } else if ch == '/' && self.peek_ahead(1) == Some('/') {
                // Line comment
                while self.pos < self.source.len() && self.source[self.pos] != '\n' {
                    self.advance();
                }
            } else if ch == '/' && self.peek_ahead(1) == Some('*') {
                // Block comment
                self.advance(); // /
                self.advance(); // *
                let mut depth = 1;
                while self.pos < self.source.len() && depth > 0 {
                    if self.source[self.pos] == '/' && self.peek_ahead(1) == Some('*') {
                        depth += 1;
                        self.advance();
                        self.advance();
                    } else if self.source[self.pos] == '*' && self.peek_ahead(1) == Some('/') {
                        depth -= 1;
                        self.advance();
                        self.advance();
                    } else {
                        self.advance();
                    }
                }
            } else {
                break;
            }
        }
    }

    fn next_token(&mut self) -> Result<SpannedToken, String> {
        let line = self.line;
        let col = self.col;
        let ch = self.peek().unwrap();

        // String literal
        if ch == '"' {
            return self.read_string(line, col);
        }

        // Number literal
        if ch.is_ascii_digit() {
            return self.read_number(line, col);
        }

        // Identifier or keyword
        if ch.is_alphabetic() || ch == '_' {
            return Ok(self.read_ident_or_keyword(line, col));
        }

        // Punctuation and operators
        let token = match ch {
            '(' => { self.advance(); Token::LParen }
            ')' => { self.advance(); Token::RParen }
            '{' => { self.advance(); Token::LBrace }
            '}' => { self.advance(); Token::RBrace }
            '[' => { self.advance(); Token::LBracket }
            ']' => { self.advance(); Token::RBracket }
            ',' => { self.advance(); Token::Comma }
            ';' => { self.advance(); Token::Semicolon }
            ':' => { self.advance(); Token::Colon }
            '.' => { self.advance(); Token::Dot }
            '%' => { self.advance(); Token::Percent }
            '+' => { self.advance(); Token::Plus }
            '*' => { self.advance(); Token::Star }
            '/' => { self.advance(); Token::Slash }
            '-' => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    Token::Arrow
                } else {
                    Token::Minus
                }
            }
            '=' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Token::EqEq
                } else {
                    Token::Eq
                }
            }
            '!' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Token::BangEq
                } else {
                    Token::Bang
                }
            }
            '<' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Token::LtEq
                } else {
                    Token::Lt
                }
            }
            '>' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Token::GtEq
                } else {
                    Token::Gt
                }
            }
            '&' => {
                self.advance();
                if self.peek() == Some('&') {
                    self.advance();
                    Token::AmpAmp
                } else {
                    return Err(format!("{}:{}: unexpected character '&' (did you mean '&&'?)", line, col));
                }
            }
            '|' => {
                self.advance();
                if self.peek() == Some('|') {
                    self.advance();
                    Token::PipePipe
                } else {
                    return Err(format!("{}:{}: unexpected character '|' (did you mean '||'?)", line, col));
                }
            }
            _ => {
                return Err(format!("{}:{}: unexpected character '{}'", line, col, ch));
            }
        };

        Ok(SpannedToken { token, line, col })
    }

    fn read_string(&mut self, line: usize, col: usize) -> Result<SpannedToken, String> {
        self.advance(); // opening quote
        let mut s = String::new();
        loop {
            match self.peek() {
                None => return Err(format!("{}:{}: unterminated string literal", line, col)),
                Some('"') => {
                    self.advance();
                    break;
                }
                Some('\\') => {
                    self.advance();
                    match self.peek() {
                        Some('n') => { self.advance(); s.push('\n'); }
                        Some('t') => { self.advance(); s.push('\t'); }
                        Some('\\') => { self.advance(); s.push('\\'); }
                        Some('"') => { self.advance(); s.push('"'); }
                        Some('0') => { self.advance(); s.push('\0'); }
                        Some(c) => {
                            return Err(format!("{}:{}: unknown escape sequence '\\{}'", self.line, self.col, c));
                        }
                        None => return Err(format!("{}:{}: unterminated string escape", self.line, self.col)),
                    }
                }
                Some(c) => {
                    self.advance();
                    s.push(c);
                }
            }
        }
        Ok(SpannedToken { token: Token::StringLit(s), line, col })
    }

    fn read_number(&mut self, line: usize, col: usize) -> Result<SpannedToken, String> {
        let mut num_str = String::new();
        let mut is_float = false;

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                if c != '_' {
                    num_str.push(c);
                }
                self.advance();
            } else if c == '.' && !is_float {
                // Check if it's really a decimal point (not a method call)
                if let Some(next) = self.peek_ahead(1) {
                    if next.is_ascii_digit() {
                        is_float = true;
                        num_str.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if is_float {
            let val: f64 = num_str.parse().map_err(|e| {
                format!("{}:{}: invalid float literal '{}': {}", line, col, num_str, e)
            })?;
            Ok(SpannedToken { token: Token::FloatLit(val), line, col })
        } else {
            let val: i64 = num_str.parse().map_err(|e| {
                format!("{}:{}: invalid integer literal '{}': {}", line, col, num_str, e)
            })?;
            Ok(SpannedToken { token: Token::IntLit(val), line, col })
        }
    }

    fn read_ident_or_keyword(&mut self, line: usize, col: usize) -> SpannedToken {
        let mut ident = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                ident.push(c);
                self.advance();
            } else {
                break;
            }
        }

        let token = match ident.as_str() {
            "let" => Token::Let,
            "fn" => Token::Fn,
            "struct" => Token::Struct,
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
            "loop" => Token::Loop,
            "break" => Token::Break,
            "return" => Token::Return,
            "true" => Token::BoolLit(true),
            "false" => Token::BoolLit(false),
            _ => Token::Ident(ident),
        };

        SpannedToken { token, line, col }
    }
}
