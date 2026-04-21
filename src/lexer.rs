use crate::error::{CrustError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    Char(char),
    True,
    False,

    // Identifier and macro name (ident!)
    Ident(String),
    MacroName(String),

    // Keywords
    As, Break, Const, Continue, Else, Enum, Fn, For, If, Impl,
    In, Let, Loop, Match, Mod, Move, Mut, Pub, Ref, Return,
    SelfKw, Static, Struct, Trait, Type, Use, Where, While,

    // Operators
    Plus, Minus, Star, Slash, Percent,
    Caret, And, Or,
    PlusEq, MinusEq, StarEq, SlashEq, PercentEq,
    CaretEq, AndEq, OrEq,
    Shl, Shr, ShlEq, ShrEq,
    Eq, EqEq, Ne,
    Lt, Le, Gt, Ge,
    AndAnd, OrOr, Not,
    Arrow, FatArrow,

    // Punctuation
    At, Dot, DotDot, DotDotEq, Comma, Semi, Colon, ColonColon,
    Hash, Dollar, Question, Underscore,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,

    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
}

pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Lexer { chars: source.chars().collect(), pos: 0, line: 1 }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let done = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if done { break; }
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if let Some(ch) = c {
            self.pos += 1;
            if ch == '\n' { self.line += 1; }
        }
        c
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) { self.advance(); true } else { false }
    }

    fn skip_whitespace(&mut self) {
        while self.peek().map_or(false, |c| c.is_whitespace()) { self.advance(); }
    }

    fn skip_line_comment(&mut self) {
        while self.peek().map_or(false, |c| c != '\n') { self.advance(); }
    }

    fn skip_block_comment(&mut self) -> Result<()> {
        let line = self.line;
        let mut depth = 1usize;
        loop {
            match self.advance() {
                None => return Err(CrustError::parse("unterminated block comment", line)),
                Some('/') if self.peek() == Some('*') => { self.advance(); depth += 1; }
                Some('*') if self.peek() == Some('/') => {
                    self.advance();
                    depth -= 1;
                    if depth == 0 { break; }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn read_string(&mut self) -> Result<String> {
        let line = self.line;
        let mut s = String::new();
        loop {
            match self.advance() {
                None | Some('\n') => return Err(CrustError::parse("unterminated string", line)),
                Some('"') => break,
                Some('\\') => s.push(self.read_escape(line)?),
                Some(c) => s.push(c),
            }
        }
        Ok(s)
    }

    fn read_escape(&mut self, line: usize) -> Result<char> {
        match self.advance() {
            Some('n') => Ok('\n'),
            Some('t') => Ok('\t'),
            Some('r') => Ok('\r'),
            Some('\\') => Ok('\\'),
            Some('"') => Ok('"'),
            Some('\'') => Ok('\''),
            Some('0') => Ok('\0'),
            Some(c) => Ok(c),
            None => Err(CrustError::parse("unterminated escape", line)),
        }
    }

    fn read_char_lit(&mut self) -> Result<char> {
        let line = self.line;
        let c = match self.advance() {
            Some('\\') => self.read_escape(line)?,
            Some(c) => c,
            None => return Err(CrustError::parse("unterminated char literal", line)),
        };
        if !self.eat('\'') {
            return Err(CrustError::parse("unterminated char literal", line));
        }
        Ok(c)
    }

    fn read_number(&mut self, first: char) -> Result<TokenKind> {
        let mut s = String::from(first);
        let mut is_float = false;

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                s.push(c); self.advance();
            } else if c == '.' && self.peek2().map_or(false, |c2| c2.is_ascii_digit()) {
                s.push(c); self.advance(); is_float = true;
            } else if (c == 'e' || c == 'E') && !is_float {
                // check it's actually an exponent, not a suffix
                if self.peek2().map_or(false, |c2| c2.is_ascii_digit() || c2 == '+' || c2 == '-') {
                    s.push(c); self.advance(); is_float = true;
                    if self.peek() == Some('+') || self.peek() == Some('-') {
                        s.push(self.advance().unwrap());
                    }
                } else { break; }
            } else { break; }
        }
        // strip numeric suffixes: i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64
        if self.peek().map_or(false, |c| c.is_alphabetic() || c == '_') {
            while self.peek().map_or(false, |c| c.is_alphanumeric() || c == '_') {
                let c = self.advance().unwrap();
                if c == 'f' || c == '.' { is_float = true; }
            }
        }

        let clean = s.replace('_', "");
        if is_float {
            clean.parse::<f64>().map(TokenKind::Float)
                .map_err(|_| CrustError::parse(format!("invalid float '{}'", clean), self.line))
        } else {
            clean.parse::<i64>().map(TokenKind::Int)
                .map_err(|_| CrustError::parse(format!("invalid integer '{}'", clean), self.line))
        }
    }

    fn read_ident(&mut self, first: char) -> String {
        let mut s = String::from(first);
        while self.peek().map_or(false, |c| c.is_alphanumeric() || c == '_') {
            s.push(self.advance().unwrap());
        }
        s
    }

    fn keyword_or_ident(s: String) -> TokenKind {
        match s.as_str() {
            "as"       => TokenKind::As,
            "break"    => TokenKind::Break,
            "const"    => TokenKind::Const,
            "continue" => TokenKind::Continue,
            "else"     => TokenKind::Else,
            "enum"     => TokenKind::Enum,
            "false"    => TokenKind::False,
            "fn"       => TokenKind::Fn,
            "for"      => TokenKind::For,
            "if"       => TokenKind::If,
            "impl"     => TokenKind::Impl,
            "in"       => TokenKind::In,
            "let"      => TokenKind::Let,
            "loop"     => TokenKind::Loop,
            "match"    => TokenKind::Match,
            "mod"      => TokenKind::Mod,
            "move"     => TokenKind::Move,
            "mut"      => TokenKind::Mut,
            "pub"      => TokenKind::Pub,
            "ref"      => TokenKind::Ref,
            "return"   => TokenKind::Return,
            "self" | "Self" => TokenKind::SelfKw,
            "static"   => TokenKind::Static,
            "struct"   => TokenKind::Struct,
            "trait"    => TokenKind::Trait,
            "true"     => TokenKind::True,
            "type"     => TokenKind::Type,
            "use"      => TokenKind::Use,
            "where"    => TokenKind::Where,
            "while"    => TokenKind::While,
            "_"        => TokenKind::Underscore,
            _          => TokenKind::Ident(s),
        }
    }

    fn next_token(&mut self) -> Result<Token> {
        loop {
            self.skip_whitespace();
            let line = self.line;

            let c = match self.advance() {
                None => return Ok(Token { kind: TokenKind::Eof, line }),
                Some(c) => c,
            };

            let kind = match c {
                '/' if self.peek() == Some('/') => { self.skip_line_comment(); continue; }
                '/' if self.peek() == Some('*') => { self.advance(); self.skip_block_comment()?; continue; }

                '"' => TokenKind::Str(self.read_string()?),

                '\'' => {
                    // Determine: char literal or lifetime annotation
                    let next = self.peek();
                    let next2 = self.peek2();
                    if next.map_or(false, |c| c != '\\' && c != '\'')
                        && next2 == Some('\'')
                    {
                        // 'x' - char literal
                        TokenKind::Char(self.read_char_lit()?)
                    } else if next == Some('\\') {
                        // '\n' style char literal
                        TokenKind::Char(self.read_char_lit()?)
                    } else {
                        // lifetime: 'a, 'static, etc. — skip, emit nothing useful
                        // consume the lifetime name so it doesn't pollute the stream
                        if next.map_or(false, |c| c.is_alphabetic() || c == '_') {
                            while self.peek().map_or(false, |c| c.is_alphanumeric() || c == '_') {
                                self.advance();
                            }
                        }
                        continue; // skip lifetime tokens entirely
                    }
                }

                c if c.is_ascii_digit() => self.read_number(c)?,

                '#' => {
                    // Skip attributes: #[...] and #![...]
                    self.skip_whitespace();
                    if self.eat('!') { self.skip_whitespace(); }
                    if self.eat('[') {
                        let mut depth = 1;
                        while depth > 0 {
                            match self.advance() {
                                None => break,
                                Some('[') => depth += 1,
                                Some(']') => depth -= 1,
                                _ => {}
                            }
                        }
                    }
                    continue;
                }

                c if c.is_alphabetic() || c == '_' => {
                    let s = self.read_ident(c);
                    // Check for macro invocation: ident!(  ident![  ident!{
                    if self.peek() == Some('!')
                        && matches!(self.peek2(), Some('(' | '[' | '{'))
                    {
                        self.advance(); // consume !
                        TokenKind::MacroName(s)
                    } else {
                        Self::keyword_or_ident(s)
                    }
                }

                '(' => TokenKind::LParen,
                ')' => TokenKind::RParen,
                '{' => TokenKind::LBrace,
                '}' => TokenKind::RBrace,
                '[' => TokenKind::LBracket,
                ']' => TokenKind::RBracket,
                ';' => TokenKind::Semi,
                ',' => TokenKind::Comma,
                '@' => TokenKind::At,
                '$' => TokenKind::Dollar,
                '?' => TokenKind::Question,
                '~' => { continue; } // tilde is unused in modern Rust

                '.' => {
                    if self.peek() == Some('.') {
                        self.advance();
                        if self.eat('=') { TokenKind::DotDotEq }
                        else { TokenKind::DotDot }
                    } else {
                        TokenKind::Dot
                    }
                }

                ':' => {
                    if self.eat(':') { TokenKind::ColonColon }
                    else { TokenKind::Colon }
                }

                '=' => {
                    if self.eat('=') { TokenKind::EqEq }
                    else if self.eat('>') { TokenKind::FatArrow }
                    else { TokenKind::Eq }
                }

                '!' => {
                    if self.eat('=') { TokenKind::Ne }
                    else { TokenKind::Not }
                }

                '<' => {
                    if self.peek() == Some('<') {
                        self.advance();
                        if self.eat('=') { TokenKind::ShlEq } else { TokenKind::Shl }
                    } else if self.eat('=') { TokenKind::Le }
                    else { TokenKind::Lt }
                }

                '>' => {
                    if self.peek() == Some('>') {
                        self.advance();
                        if self.eat('=') { TokenKind::ShrEq } else { TokenKind::Shr }
                    } else if self.eat('=') { TokenKind::Ge }
                    else { TokenKind::Gt }
                }

                '+' => { if self.eat('=') { TokenKind::PlusEq } else { TokenKind::Plus } }
                '-' => {
                    if self.eat('=') { TokenKind::MinusEq }
                    else if self.eat('>') { TokenKind::Arrow }
                    else { TokenKind::Minus }
                }
                '*' => { if self.eat('=') { TokenKind::StarEq } else { TokenKind::Star } }
                '/' => { if self.eat('=') { TokenKind::SlashEq } else { TokenKind::Slash } }
                '%' => { if self.eat('=') { TokenKind::PercentEq } else { TokenKind::Percent } }
                '^' => { if self.eat('=') { TokenKind::CaretEq } else { TokenKind::Caret } }
                '&' => {
                    if self.eat('&') { TokenKind::AndAnd }
                    else if self.eat('=') { TokenKind::AndEq }
                    else { TokenKind::And }
                }
                '|' => {
                    if self.eat('|') { TokenKind::OrOr }
                    else if self.eat('=') { TokenKind::OrEq }
                    else { TokenKind::Or }
                }

                c => return Err(CrustError::parse(format!("unexpected character {:?}", c), line)),
            };

            return Ok(Token { kind, line });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(src: &str) -> Vec<TokenKind> {
        Lexer::new(src).tokenize().unwrap()
            .into_iter()
            .map(|t| t.kind)
            .filter(|k| *k != TokenKind::Eof)
            .collect()
    }

    #[test]
    fn basic_tokens() {
        let kinds = lex("let x = 42;");
        assert_eq!(kinds, vec![
            TokenKind::Let,
            TokenKind::Ident("x".into()),
            TokenKind::Eq,
            TokenKind::Int(42),
            TokenKind::Semi,
        ]);
    }

    #[test]
    fn string_literal() {
        let kinds = lex(r#""hello\nworld""#);
        assert_eq!(kinds, vec![TokenKind::Str("hello\nworld".into())]);
    }

    #[test]
    fn float_literal() {
        let kinds = lex("3.14");
        assert_eq!(kinds, vec![TokenKind::Float(3.14)]);
    }

    #[test]
    fn macro_name() {
        let kinds = lex("println!(");
        assert_eq!(kinds[0], TokenKind::MacroName("println".into()));
    }

    #[test]
    fn arrow_and_fat_arrow() {
        let kinds = lex("-> =>");
        assert_eq!(kinds, vec![TokenKind::Arrow, TokenKind::FatArrow]);
    }

    #[test]
    fn skips_line_comments() {
        let kinds = lex("let x = 1; // comment\nlet y = 2;");
        assert!(kinds.contains(&TokenKind::Ident("x".into())));
        assert!(kinds.contains(&TokenKind::Ident("y".into())));
        assert!(!kinds.iter().any(|k| matches!(k, TokenKind::Ident(s) if s == "comment")));
    }

    #[test]
    fn skips_attributes() {
        let kinds = lex("#[derive(Clone)] struct Foo {}");
        assert!(kinds.contains(&TokenKind::Struct));
        assert!(!kinds.iter().any(|k| matches!(k, TokenKind::MacroName(s) if s == "derive")));
    }
}
