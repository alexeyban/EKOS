//! EKL grammar, AST, and a hand-written lexer + recursive-descent parser (RFC 0010).
//!
//! No `pest`/`nom` dependency: the grammar is six flat clause types with no
//! recursive expression precedence, so a hand-written parser stays simple and
//! is easy to fuzz for "never panics, only returns `ParseError`".

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Entity {
    Object,
    Relationship,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
    Contains,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Str(String),
    Num(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Predicate {
    pub field: String,
    pub op: Op,
    pub value: Literal,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Order {
    Asc,
    Desc,
}

/// The full parsed shape of one EKL query.
#[derive(Debug, Clone, PartialEq)]
pub struct EklAst {
    pub entity: Entity,
    pub predicates: Vec<Predicate>,
    pub from: Option<String>,
    pub returns: Vec<String>,
    pub order_by: Option<(String, Order)>,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "parse error at byte {}: {}", self.position, self.message)
    }
}

impl std::error::Error for ParseError {}

// ── Lexer ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Str(String),
    Num(f64),
    Comma,
}

struct Lexer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    fn tokenize(mut self) -> Result<Vec<(Token, usize)>, ParseError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            if self.pos >= self.bytes.len() {
                break;
            }
            let start = self.pos;
            let c = self.bytes[self.pos];

            if c == b',' {
                self.pos += 1;
                tokens.push((Token::Comma, start));
            } else if c == b'\'' {
                tokens.push((Token::Str(self.read_string()?), start));
            } else if c.is_ascii_digit() {
                tokens.push((Token::Num(self.read_number()?), start));
            } else if c.is_ascii_alphabetic() || c == b'_' {
                tokens.push((Token::Ident(self.read_ident()), start));
            } else if let Some(op_len) = self.match_symbol_op() {
                let text = &self.input[self.pos..self.pos + op_len];
                tokens.push((Token::Ident(text.to_string()), start));
                self.pos += op_len;
            } else {
                return Err(ParseError {
                    message: format!("unexpected character '{}'", c as char),
                    position: start,
                });
            }
        }
        Ok(tokens)
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn match_symbol_op(&self) -> Option<usize> {
        let rest = &self.input[self.pos..];
        for op in ["!=", ">=", "<=", "=", ">", "<"] {
            if rest.starts_with(op) {
                return Some(op.len());
            }
        }
        None
    }

    fn read_string(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            if self.pos >= self.bytes.len() {
                return Err(ParseError {
                    message: "unterminated string literal".into(),
                    position: start,
                });
            }
            let c = self.bytes[self.pos];
            if c == b'\'' {
                self.pos += 1;
                return Ok(out);
            }
            out.push(c as char);
            self.pos += 1;
        }
    }

    fn read_number(&mut self) -> Result<f64, ParseError> {
        let start = self.pos;
        while self.pos < self.bytes.len()
            && (self.bytes[self.pos].is_ascii_digit() || self.bytes[self.pos] == b'.')
        {
            self.pos += 1;
        }
        self.input[start..self.pos]
            .parse::<f64>()
            .map_err(|_| ParseError {
                message: "invalid number literal".into(),
                position: start,
            })
    }

    fn read_ident(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.bytes.len()
            && (self.bytes[self.pos].is_ascii_alphanumeric() || self.bytes[self.pos] == b'_')
        {
            self.pos += 1;
        }
        self.input[start..self.pos].to_string()
    }
}

// ── Parser ───────────────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<(Token, usize)>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|(t, _)| t)
    }

    fn peek_pos(&self) -> usize {
        self.tokens
            .get(self.pos)
            .map(|(_, p)| *p)
            .unwrap_or_else(|| self.tokens.last().map(|(_, p)| *p + 1).unwrap_or(0))
    }

    fn advance(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).map(|(t, _)| t.clone());
        self.pos += 1;
        t
    }

    fn expect_keyword(&mut self, kw: &str) -> Result<(), ParseError> {
        let pos = self.peek_pos();
        match self.advance() {
            Some(Token::Ident(s)) if s.eq_ignore_ascii_case(kw) => Ok(()),
            other => Err(ParseError {
                message: format!("expected '{kw}', found {}", describe(other.as_ref())),
                position: pos,
            }),
        }
    }

    fn peek_keyword(&self, kw: &str) -> bool {
        matches!(self.peek(), Some(Token::Ident(s)) if s.eq_ignore_ascii_case(kw))
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        let pos = self.peek_pos();
        match self.advance() {
            Some(Token::Ident(s)) => Ok(s),
            other => Err(ParseError {
                message: format!("expected identifier, found {}", describe(other.as_ref())),
                position: pos,
            }),
        }
    }

    fn expect_string(&mut self) -> Result<String, ParseError> {
        let pos = self.peek_pos();
        match self.advance() {
            Some(Token::Str(s)) => Ok(s),
            other => Err(ParseError {
                message: format!(
                    "expected string literal, found {}",
                    describe(other.as_ref())
                ),
                position: pos,
            }),
        }
    }

    fn expect_num(&mut self) -> Result<f64, ParseError> {
        let pos = self.peek_pos();
        match self.advance() {
            Some(Token::Num(n)) => Ok(n),
            other => Err(ParseError {
                message: format!("expected number, found {}", describe(other.as_ref())),
                position: pos,
            }),
        }
    }

    fn parse_query(&mut self) -> Result<EklAst, ParseError> {
        self.expect_keyword("FIND")?;
        let entity = self.parse_entity()?;

        let mut predicates = Vec::new();
        let mut from = None;
        let mut returns = Vec::new();
        let mut order_by = None;
        let mut limit = None;

        if self.peek_keyword("WHERE") {
            self.advance();
            predicates.push(self.parse_predicate()?);
            while self.peek_keyword("AND") {
                self.advance();
                predicates.push(self.parse_predicate()?);
            }
        }

        // FROM / RETURN / ORDER BY / LIMIT may appear in any order.
        loop {
            if self.peek_keyword("FROM") {
                self.advance();
                from = Some(self.expect_string()?);
            } else if self.peek_keyword("RETURN") {
                self.advance();
                returns.push(self.expect_ident()?);
                while matches!(self.peek(), Some(Token::Comma)) {
                    self.advance();
                    returns.push(self.expect_ident()?);
                }
            } else if self.peek_keyword("ORDER") {
                self.advance();
                self.expect_keyword("BY")?;
                let field = self.expect_ident()?;
                let dir = if self.peek_keyword("DESC") {
                    self.advance();
                    Order::Desc
                } else if self.peek_keyword("ASC") {
                    self.advance();
                    Order::Asc
                } else {
                    Order::Asc
                };
                order_by = Some((field, dir));
            } else if self.peek_keyword("LIMIT") {
                self.advance();
                limit = Some(self.expect_num()? as u64);
            } else {
                break;
            }
        }

        if self.pos != self.tokens.len() {
            let pos = self.peek_pos();
            return Err(ParseError {
                message: format!("unexpected trailing token {}", describe(self.peek())),
                position: pos,
            });
        }

        Ok(EklAst {
            entity,
            predicates,
            from,
            returns,
            order_by,
            limit,
        })
    }

    fn parse_entity(&mut self) -> Result<Entity, ParseError> {
        let pos = self.peek_pos();
        let ident = self.expect_ident()?;
        if ident.eq_ignore_ascii_case("Object") {
            Ok(Entity::Object)
        } else if ident.eq_ignore_ascii_case("Relationship") {
            Ok(Entity::Relationship)
        } else {
            Err(ParseError {
                message: format!("unknown entity '{ident}'"),
                position: pos,
            })
        }
    }

    fn parse_predicate(&mut self) -> Result<Predicate, ParseError> {
        let field = self.expect_ident()?;
        let op = self.parse_op()?;
        let value = self.parse_literal()?;
        Ok(Predicate { field, op, value })
    }

    fn parse_op(&mut self) -> Result<Op, ParseError> {
        let pos = self.peek_pos();
        match self.advance() {
            Some(Token::Ident(s)) => match s.as_str() {
                "=" => Ok(Op::Eq),
                "!=" => Ok(Op::Ne),
                ">" => Ok(Op::Gt),
                "<" => Ok(Op::Lt),
                ">=" => Ok(Op::Ge),
                "<=" => Ok(Op::Le),
                _ if s.eq_ignore_ascii_case("CONTAINS") => Ok(Op::Contains),
                other => Err(ParseError {
                    message: format!("unknown operator '{other}'"),
                    position: pos,
                }),
            },
            other => Err(ParseError {
                message: format!("expected operator, found {}", describe(other.as_ref())),
                position: pos,
            }),
        }
    }

    fn parse_literal(&mut self) -> Result<Literal, ParseError> {
        let pos = self.peek_pos();
        match self.advance() {
            Some(Token::Str(s)) => Ok(Literal::Str(s)),
            Some(Token::Num(n)) => Ok(Literal::Num(n)),
            other => Err(ParseError {
                message: format!("expected literal, found {}", describe(other.as_ref())),
                position: pos,
            }),
        }
    }
}

fn describe(t: Option<&Token>) -> String {
    match t {
        None => "end of input".to_string(),
        Some(Token::Ident(s)) => format!("'{s}'"),
        Some(Token::Str(s)) => format!("'{s}' (string)"),
        Some(Token::Num(n)) => format!("{n} (number)"),
        Some(Token::Comma) => "','".to_string(),
    }
}

/// Parse an EKL query string into an `EklAst`. Never panics — malformed input
/// always yields `Err(ParseError)`.
pub fn ekl_parse(input: &str) -> Result<EklAst, ParseError> {
    let tokens = Lexer::new(input).tokenize()?;
    Parser::new(tokens).parse_query()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_find_object() {
        let ast = ekl_parse("FIND Object WHERE kind = 'Table'").unwrap();
        assert_eq!(ast.entity, Entity::Object);
        assert_eq!(
            ast.predicates,
            vec![Predicate {
                field: "kind".into(),
                op: Op::Eq,
                value: Literal::Str("Table".into())
            }]
        );
    }

    #[test]
    fn parses_return_clause() {
        let ast = ekl_parse("FIND Object WHERE kind = 'Table' RETURN name, evidence").unwrap();
        assert_eq!(ast.returns, vec!["name", "evidence"]);
    }

    #[test]
    fn parses_relationship_with_from() {
        let ast = ekl_parse("FIND Relationship WHERE kind = 'ForeignKey' FROM 'orders'").unwrap();
        assert_eq!(ast.entity, Entity::Relationship);
        assert_eq!(ast.from, Some("orders".to_string()));
    }

    #[test]
    fn parses_order_by_and_limit() {
        let ast = ekl_parse("FIND Object WHERE kind = 'Table' ORDER BY name LIMIT 1").unwrap();
        assert_eq!(ast.order_by, Some(("name".to_string(), Order::Asc)));
        assert_eq!(ast.limit, Some(1));
    }

    #[test]
    fn parses_order_by_desc() {
        let ast = ekl_parse("FIND Object ORDER BY name DESC").unwrap();
        assert_eq!(ast.order_by, Some(("name".to_string(), Order::Desc)));
    }

    #[test]
    fn parses_and_chained_predicates() {
        let ast = ekl_parse("FIND Object WHERE kind = 'Table' AND name CONTAINS 'order'").unwrap();
        assert_eq!(ast.predicates.len(), 2);
        assert_eq!(ast.predicates[1].op, Op::Contains);
    }

    #[test]
    fn parses_query_with_no_where() {
        let ast = ekl_parse("FIND Object FROM 'orders'").unwrap();
        assert!(ast.predicates.is_empty());
        assert_eq!(ast.from, Some("orders".to_string()));
    }

    #[test]
    fn parses_numeric_comparison_operators() {
        for (text, expected) in [
            ("FIND Object WHERE x > 1", Op::Gt),
            ("FIND Object WHERE x < 1", Op::Lt),
            ("FIND Object WHERE x >= 1", Op::Ge),
            ("FIND Object WHERE x <= 1", Op::Le),
            ("FIND Object WHERE x != 1", Op::Ne),
        ] {
            let ast = ekl_parse(text).unwrap();
            assert_eq!(ast.predicates[0].op, expected, "for {text}");
        }
    }

    #[test]
    fn rejects_unknown_entity() {
        assert!(ekl_parse("FIND Widget").is_err());
    }

    #[test]
    fn rejects_missing_find_keyword() {
        assert!(ekl_parse("Object WHERE kind = 'Table'").is_err());
    }

    #[test]
    fn rejects_unterminated_string() {
        assert!(ekl_parse("FIND Object WHERE kind = 'Table").is_err());
    }

    #[test]
    fn rejects_trailing_garbage() {
        assert!(ekl_parse("FIND Object WHERE kind = 'Table' ZZZ").is_err());
    }

    #[test]
    fn fuzz_random_strings_never_panic() {
        let seeds = [
            "",
            "FIND",
            "FIND Object WHERE",
            "'''",
            "!!!===",
            "FIND Object WHERE a = ",
            "FIND Object LIMIT abc",
            "\u{0}\u{1}\u{2}",
            "FIND Object WHERE a = 'unterminated",
            "FIND Relationship FROM",
            ",,,,",
            "FIND Object RETURN ,",
        ];
        for s in seeds {
            let _ = ekl_parse(s); // must not panic
        }
    }
}
