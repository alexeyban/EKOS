//! RFC 0209 — a hand-written Java tokenizer and declaration-level parser.
//!
//! No pure-Rust Java parser exists (`tree-sitter-java` needs a C toolchain, which the workspace's
//! parser policy rejects). Unlike Perl, Java is lexically unambiguous: comments, string, char and
//! text-block literals, and braces can be told apart without context, so a real tokenizer plus
//! bracket matching gives exact spans. On top of it this module reads **declarations** — package,
//! imports, types (nested too), methods, constructors, fields — and leaves method bodies as token
//! ranges for later scanners (`java_sql`, `app_model`). Expressions are never parsed.
//!
//! Every file comes back with a [`FileStatus`]: `Parsed` when all brackets matched and every member
//! header was classified, `Partial` when some member header was not (counted), `Skipped` when the
//! brackets do not balance — then nothing is guessed and no declarations are returned.

use std::collections::HashMap;

/// What a token is. Comments are not tokens; they are kept apart in [`JavaFile::comments`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokKind {
    Ident,
    /// A string or text-block literal; `text` is its decoded content.
    Str,
    Char,
    Num,
    Punct,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokKind,
    pub text: String,
    pub line: u32,
}

impl Token {
    pub fn is(&self, p: &str) -> bool {
        self.kind != TokKind::Str && self.kind != TokKind::Char && self.text == p
    }
    pub fn is_ident(&self) -> bool {
        self.kind == TokKind::Ident
    }
}

/// A comment as written, with the lines it covers. `doc` is true for `/** … */`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub start_line: u32,
    pub end_line: u32,
    pub doc: bool,
    pub text: String,
}

/// Tokens plus comments. A Javadoc comment is also indexed by the token that follows it, which is
/// how a declaration finds its own documentation.
#[derive(Debug, Default)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
    pub doc_before: HashMap<usize, usize>,
}

const MODIFIERS: &[&str] = &[
    "public",
    "protected",
    "private",
    "static",
    "final",
    "abstract",
    "native",
    "synchronized",
    "transient",
    "volatile",
    "strictfp",
    "default",
    "sealed",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
}

impl TypeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Enum => "enum",
            Self::Record => "record",
            Self::Annotation => "annotation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// As written: `Override`, `org.adempiere.base.Model`.
    pub name: String,
    /// The argument list's tokens joined, without the parentheses; empty when there is none.
    pub args: String,
}

impl Annotation {
    /// The simple name: `Model` for `org.adempiere.base.Model`.
    pub fn simple_name(&self) -> &str {
        self.name.rsplit('.').next().unwrap_or(&self.name)
    }

    /// The string literal given for `key` (`table="C_Invoice"` → `C_Invoice`), or for a single
    /// unnamed argument when `key` is `value`.
    pub fn string_arg(&self, key: &str) -> Option<String> {
        let lexed = lex(&self.args);
        let t = &lexed.tokens;
        for (i, tok) in t.iter().enumerate() {
            if tok.kind == TokKind::Str {
                let named = i >= 2 && t[i - 1].is("=") && t[i - 2].is_ident();
                if (named && t[i - 2].text == key) || (!named && key == "value" && i == 0) {
                    return Some(tok.text.clone());
                }
            }
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodKind {
    Method,
    Constructor,
    /// No body: abstract, interface or native.
    Abstract,
}

impl MethodKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Method => "method",
            Self::Constructor => "constructor",
            Self::Abstract => "abstract",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub ty: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodDecl {
    pub name: String,
    pub kind: MethodKind,
    pub modifiers: Vec<String>,
    pub annotations: Vec<Annotation>,
    /// `None` for a constructor.
    pub return_type: Option<String>,
    pub params: Vec<Param>,
    /// The header as written, whitespace-normalised, without annotations or the body.
    pub signature: String,
    pub doc: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    /// Token indices of the body's `{` and `}`.
    pub body: Option<(usize, usize)>,
}

impl MethodDecl {
    /// `name(T1,T2)` — the identity of an overload.
    pub fn key(&self) -> String {
        let tys: Vec<&str> = self.params.iter().map(|p| p.ty.as_str()).collect();
        format!("{}({})", self.name, tys.join(","))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDecl {
    pub name: String,
    pub ty: String,
    pub modifiers: Vec<String>,
    pub doc: Option<String>,
    pub line: u32,
    /// Token range `[start, end)` of the initialiser expression.
    pub init: Option<(usize, usize)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDecl {
    pub name: String,
    /// `pkg.Outer.Inner`; just `Outer.Inner` without a package.
    pub qualified: String,
    pub kind: TypeKind,
    pub modifiers: Vec<String>,
    pub annotations: Vec<Annotation>,
    /// Superclass for a class; super-interfaces for an interface. Generic arguments stripped.
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub doc: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    /// Index of the enclosing type in [`JavaFile::types`].
    pub parent: Option<usize>,
    pub methods: Vec<MethodDecl>,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub path: String,
    pub is_static: bool,
    pub wildcard: bool,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Parsed,
    Partial,
    Skipped,
}

impl FileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parsed => "parsed",
            Self::Partial => "partial",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug)]
pub struct JavaFile {
    pub package: Option<String>,
    pub imports: Vec<Import>,
    /// Every type, outer before inner.
    pub types: Vec<TypeDecl>,
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
    pub status: FileStatus,
    pub unclassified_members: usize,
}

// ── Lexer ────────────────────────────────────────────────────────────────────

/// Tokenize Java source. Never fails: an unterminated literal or comment runs to end of input.
pub fn lex(source: &str) -> Lexed {
    let mut l = Lexer {
        c: source.chars().collect(),
        i: 0,
        line: 1,
        out: Lexed::default(),
        pending_doc: None,
    };
    while l.i < l.c.len() {
        let ch = l.c[l.i];
        if ch == '\n' {
            l.line += 1;
            l.i += 1;
        } else if ch.is_whitespace() {
            l.i += 1;
        } else if ch == '/' && l.at(1) == Some('/') {
            l.line_comment();
        } else if ch == '/' && l.at(1) == Some('*') {
            l.block_comment();
        } else {
            let line = l.line;
            let (kind, text) = l.token(ch);
            l.push(kind, text, line);
        }
    }
    l.out
}

/// A cursor over the source's characters.
struct Lexer {
    c: Vec<char>,
    i: usize,
    line: u32,
    out: Lexed,
    /// The Javadoc comment waiting for the next token.
    pending_doc: Option<usize>,
}

impl Lexer {
    fn at(&self, off: usize) -> Option<char> {
        self.c.get(self.i + off).copied()
    }

    fn text(&self, from: usize) -> String {
        self.c[from..self.i].iter().collect()
    }

    fn push(&mut self, kind: TokKind, text: String, line: u32) {
        if let Some(doc) = self.pending_doc.take() {
            self.out.doc_before.insert(self.out.tokens.len(), doc);
        }
        self.out.tokens.push(Token { kind, text, line });
    }

    fn line_comment(&mut self) {
        let start = self.i;
        while self.i < self.c.len() && self.c[self.i] != '\n' {
            self.i += 1;
        }
        let text = self.text(start);
        self.out.comments.push(Comment {
            start_line: self.line,
            end_line: self.line,
            doc: false,
            text,
        });
    }

    fn block_comment(&mut self) {
        let start = self.i;
        let start_line = self.line;
        let doc = self.at(2) == Some('*') && self.at(3) != Some('/');
        self.i += 2;
        while self.i < self.c.len() && !(self.c[self.i] == '*' && self.at(1) == Some('/')) {
            if self.c[self.i] == '\n' {
                self.line += 1;
            }
            self.i += 1;
        }
        self.i = (self.i + 2).min(self.c.len());
        let text = self.text(start);
        self.out.comments.push(Comment {
            start_line,
            end_line: self.line,
            doc,
            text,
        });
        if doc {
            self.pending_doc = Some(self.out.comments.len() - 1);
        }
    }

    fn token(&mut self, ch: char) -> (TokKind, String) {
        if ch == '"' && self.at(1) == Some('"') && self.at(2) == Some('"') {
            (TokKind::Str, self.text_block())
        } else if ch == '"' {
            (TokKind::Str, self.quoted(ch))
        } else if ch == '\'' {
            (TokKind::Char, self.quoted(ch))
        } else if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let start = self.i;
            while self.i < self.c.len()
                && (self.c[self.i].is_alphanumeric()
                    || self.c[self.i] == '_'
                    || self.c[self.i] == '$')
            {
                self.i += 1;
            }
            (TokKind::Ident, self.text(start))
        } else if ch.is_ascii_digit()
            || (ch == '.' && self.at(1).is_some_and(|d| d.is_ascii_digit()))
        {
            (TokKind::Num, self.number(ch))
        } else {
            self.i += 1;
            (TokKind::Punct, ch.to_string())
        }
    }

    /// `"…"` or `'…'`; stops at a line break, as the language does.
    fn quoted(&mut self, quote: char) -> String {
        self.i += 1;
        let mut s = String::new();
        while self.i < self.c.len() && self.c[self.i] != quote && self.c[self.i] != '\n' {
            if self.c[self.i] == '\\' && self.i + 1 < self.c.len() {
                s.push(unescape(self.c[self.i + 1]));
                self.i += 2;
                continue;
            }
            s.push(self.c[self.i]);
            self.i += 1;
        }
        self.i += 1;
        s
    }

    fn text_block(&mut self) -> String {
        self.i += 3;
        let mut s = String::new();
        while self.i < self.c.len()
            && !(self.c[self.i] == '"' && self.at(1) == Some('"') && self.at(2) == Some('"'))
        {
            if self.c[self.i] == '\\' && self.i + 1 < self.c.len() {
                s.push(unescape(self.c[self.i + 1]));
                self.i += 2;
                continue;
            }
            if self.c[self.i] == '\n' {
                self.line += 1;
            }
            s.push(self.c[self.i]);
            self.i += 1;
        }
        self.i = (self.i + 3).min(self.c.len());
        text_block(&s)
    }

    /// A numeric literal: digits, letters (hex, suffixes, exponent), `_`, `.`, and an exponent's
    /// sign.
    fn number(&mut self, first: char) -> String {
        let start = self.i;
        let hex = first == '0' && matches!(self.at(1), Some('x' | 'X'));
        while self.i < self.c.len() {
            let d = self.c[self.i];
            let exp_sign = (d == '+' || d == '-')
                && !hex
                && self.i > start
                && matches!(self.c[self.i - 1], 'e' | 'E');
            if !(d.is_ascii_alphanumeric() || d == '_' || d == '.' || exp_sign) {
                break;
            }
            self.i += 1;
        }
        self.text(start)
    }
}

fn unescape(c: char) -> char {
    match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        'b' => '\u{8}',
        'f' => '\u{c}',
        's' => ' ',
        '0' => '\0',
        other => other,
    }
}

/// A text block's content without its opening line break and common indentation.
fn text_block(raw: &str) -> String {
    let body = raw.strip_prefix('\n').unwrap_or(raw);
    let indent = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    body.lines()
        .map(|l| l.get(indent..).unwrap_or(l.trim_start()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text of a Javadoc comment without `/**`, `*/` and leading `*`s, lines joined by `\n`.
pub fn doc_text(raw: &str) -> String {
    let inner = raw
        .strip_prefix("/**")
        .unwrap_or(raw)
        .strip_suffix("*/")
        .unwrap_or(raw);
    inner
        .lines()
        .map(|l| {
            let t = l.trim();
            t.strip_prefix('*').map_or(t, str::trim_start).trim_end()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// The description part of a Javadoc: everything before the first block tag, on one line.
pub fn doc_description(doc: &str) -> String {
    let mut parts = Vec::new();
    for l in doc.lines() {
        if l.starts_with('@') {
            break;
        }
        if !l.is_empty() {
            parts.push(l);
        }
    }
    parts.join(" ")
}

/// `@param name text` lines of a Javadoc, by parameter name.
pub fn doc_params(doc: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current: Option<usize> = None;
    for l in doc.lines() {
        if let Some(rest) = l.strip_prefix("@param") {
            let rest = rest.trim();
            let (name, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            out.push((name.to_string(), text.trim().to_string()));
            current = Some(out.len() - 1);
        } else if l.starts_with('@') {
            current = None;
        } else if let Some(idx) = current
            && !l.is_empty()
        {
            let entry = &mut out[idx].1;
            if !entry.is_empty() {
                entry.push(' ');
            }
            entry.push_str(l);
        }
    }
    out
}

// ── Parser ───────────────────────────────────────────────────────────────────

/// Indices of matching `(`/`)`, `[`/`]`, `{`/`}`, or `None` when they do not balance.
fn match_brackets(tokens: &[Token]) -> Option<Vec<Option<usize>>> {
    let mut m = vec![None; tokens.len()];
    let mut stack: Vec<(usize, &str)> = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        if t.kind != TokKind::Punct {
            continue;
        }
        match t.text.as_str() {
            "(" | "[" | "{" => stack.push((i, t.text.as_str())),
            ")" | "]" | "}" => {
                let (open, kind) = stack.pop()?;
                let expected = match kind {
                    "(" => ")",
                    "[" => "]",
                    _ => "}",
                };
                if expected != t.text {
                    return None;
                }
                m[open] = Some(i);
                m[i] = Some(open);
            }
            _ => {}
        }
    }
    stack.is_empty().then_some(m)
}

struct Parser<'a> {
    t: &'a [Token],
    m: Vec<Option<usize>>,
    lexed: &'a Lexed,
    package: Option<String>,
    types: Vec<TypeDecl>,
    unclassified: usize,
}

/// Annotations, modifiers and documentation in front of a declaration.
#[derive(Default)]
struct Prefix {
    annotations: Vec<Annotation>,
    modifiers: Vec<String>,
    doc: Option<String>,
    start_line: Option<u32>,
}

/// Parse one Java compilation unit.
pub fn parse(source: &str) -> JavaFile {
    let lexed = lex(source);
    let Some(m) = match_brackets(&lexed.tokens) else {
        return JavaFile {
            package: None,
            imports: Vec::new(),
            types: Vec::new(),
            tokens: lexed.tokens,
            comments: lexed.comments,
            status: FileStatus::Skipped,
            unclassified_members: 0,
        };
    };
    let mut p = Parser {
        t: &lexed.tokens,
        m,
        lexed: &lexed,
        package: None,
        types: Vec::new(),
        unclassified: 0,
    };
    let imports = p.compilation_unit();
    let has_types = !p.types.is_empty();
    let status = if !has_types {
        // `package-info.java`, `module-info.java`: nothing to recover, nothing misread.
        if p.unclassified == 0 {
            FileStatus::Parsed
        } else {
            FileStatus::Skipped
        }
    } else if p.unclassified == 0 {
        FileStatus::Parsed
    } else {
        FileStatus::Partial
    };
    let (package, types, unclassified) = (p.package, p.types, p.unclassified);
    JavaFile {
        package,
        imports,
        types,
        tokens: lexed.tokens,
        comments: lexed.comments,
        status,
        unclassified_members: unclassified,
    }
}

impl Parser<'_> {
    fn tok(&self, i: usize) -> Option<&Token> {
        self.t.get(i)
    }

    fn is(&self, i: usize, p: &str) -> bool {
        self.tok(i).is_some_and(|t| t.is(p))
    }

    fn compilation_unit(&mut self) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut i = 0;
        while i < self.t.len() {
            if self.is(i, "package") {
                let (name, next) = self.qualified_name(i + 1);
                self.package = Some(name);
                i = self.skip_past_semicolon(next);
                continue;
            }
            if self.is(i, "import") {
                let line = self.t[i].line;
                let mut j = i + 1;
                let is_static = self.is(j, "static");
                if is_static {
                    j += 1;
                }
                let (path, next) = self.qualified_name(j);
                let wildcard = self.is(next, ".") && self.is(next + 1, "*");
                imports.push(Import {
                    path,
                    is_static,
                    wildcard,
                    line,
                });
                i = self.skip_past_semicolon(next);
                continue;
            }
            if self.is(i, ";") {
                i += 1;
                continue;
            }
            let (prefix, j) = self.prefix(i);
            if j > i && (self.is(j, "package") || self.is(j, "import")) {
                // `package-info.java`: annotations in front of `package`.
                i = j;
                continue;
            }
            if let Some(kind) = self.type_keyword(j) {
                i = self.type_decl(j, kind, prefix, None);
                continue;
            }
            // `module-info.java` and anything else at top level: not a declaration we read.
            self.unclassified += 1;
            i = self.skip_member(j.max(i + 1));
        }
        imports
    }

    /// `a.b.C` starting at `i`; returns the name and the index after it.
    fn qualified_name(&self, mut i: usize) -> (String, usize) {
        let mut name = String::new();
        while let Some(t) = self.tok(i) {
            if !t.is_ident() {
                break;
            }
            name.push_str(&t.text);
            if self.is(i + 1, ".") && self.tok(i + 2).is_some_and(Token::is_ident) {
                name.push('.');
                i += 2;
            } else {
                i += 1;
                break;
            }
        }
        (name, i)
    }

    fn skip_past_semicolon(&self, mut i: usize) -> usize {
        while i < self.t.len() && !self.is(i, ";") {
            i += 1;
        }
        i + 1
    }

    /// Skip one unclassified member: to the end of its statement or block, whichever comes first.
    fn skip_member(&self, mut i: usize) -> usize {
        while i < self.t.len() {
            if self.is(i, ";") {
                return i + 1;
            }
            if self.is(i, "}") {
                return i;
            }
            if self.is(i, "{") || self.is(i, "(") || self.is(i, "[") {
                if self.is(i, "{") {
                    return self.m[i].map_or(i + 1, |c| c + 1);
                }
                i = self.m[i].map_or(i + 1, |c| c + 1);
                continue;
            }
            i += 1;
        }
        i
    }

    /// Annotations and modifiers starting at `i`, plus the Javadoc in front of the first of them.
    fn prefix(&self, mut i: usize) -> (Prefix, usize) {
        let mut p = Prefix {
            doc: self
                .lexed
                .doc_before
                .get(&i)
                .map(|&c| doc_text(&self.lexed.comments[c].text)),
            start_line: self.tok(i).map(|t| t.line),
            ..Default::default()
        };
        loop {
            if self.is(i, "@") && !self.is(i + 1, "interface") {
                let (name, next) = self.qualified_name(i + 1);
                let mut args = String::new();
                let mut after = next;
                if self.is(next, "(")
                    && let Some(close) = self.m[next]
                {
                    args = self.join(next + 1, close);
                    after = close + 1;
                }
                p.annotations.push(Annotation { name, args });
                i = after;
                continue;
            }
            if let Some(t) = self.tok(i)
                && t.is_ident()
                && MODIFIERS.contains(&t.text.as_str())
                // `default` is a modifier only in front of an interface method's header.
                && !(t.text == "default" && (self.is(i + 1, ":") || self.is(i + 1, "->")))
            {
                p.modifiers.push(t.text.clone());
                i += 1;
                continue;
            }
            if self.is(i, "non") && self.is(i + 1, "-") && self.is(i + 2, "sealed") {
                p.modifiers.push("non-sealed".into());
                i += 3;
                continue;
            }
            return (p, i);
        }
    }

    fn type_keyword(&self, i: usize) -> Option<TypeKind> {
        let t = self.tok(i)?;
        if !t.is_ident() {
            return (t.is("@") && self.is(i + 1, "interface")).then_some(TypeKind::Annotation);
        }
        match t.text.as_str() {
            "class" => Some(TypeKind::Class),
            "interface" => Some(TypeKind::Interface),
            "enum" if self.tok(i + 1).is_some_and(Token::is_ident) => Some(TypeKind::Enum),
            // `record` is contextual: a declaration only when a name and `(`/`<` follow.
            "record"
                if self.tok(i + 1).is_some_and(Token::is_ident)
                    && (self.is(i + 2, "(") || self.is(i + 2, "<")) =>
            {
                Some(TypeKind::Record)
            }
            _ => None,
        }
    }

    /// Tokens `[from, to)` joined with the spacing a reader expects.
    fn join(&self, from: usize, to: usize) -> String {
        let mut s = String::new();
        let mut prev: Option<&Token> = None;
        for t in &self.t[from..to.min(self.t.len())] {
            if let Some(p) = prev {
                let generic_open =
                    t.is("<") && p.is_ident() && !MODIFIERS.contains(&p.text.as_str());
                let tight = matches!(t.text.as_str(), "." | "," | ")" | "]" | ">" | "(" | "[")
                    && t.kind == TokKind::Punct
                    || generic_open
                    || matches!(p.text.as_str(), "." | "(" | "[" | "<" | "@")
                        && p.kind == TokKind::Punct;
                let comma = p.is(",");
                if !tight || comma {
                    s.push(' ');
                }
            }
            match t.kind {
                TokKind::Str => {
                    s.push('"');
                    s.push_str(&t.text);
                    s.push('"');
                }
                TokKind::Char => {
                    s.push('\'');
                    s.push_str(&t.text);
                    s.push('\'');
                }
                _ => s.push_str(&t.text),
            }
            prev = Some(t);
        }
        s
    }

    /// Skip a `<…>` group starting at `i`; returns the index after its `>`.
    fn skip_angles(&self, mut i: usize) -> usize {
        let mut depth = 0i32;
        while let Some(t) = self.tok(i) {
            if t.is("<") {
                depth += 1;
            } else if t.is(">") {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            } else if t.is("(") || t.is("[") {
                i = self.m[i].unwrap_or(i);
            } else if t.is("{") || t.is(";") || t.is("}") {
                return i;
            }
            i += 1;
        }
        i
    }

    /// A comma-separated list of type names (`A, b.C<D>`) until a token in `stop`, generics
    /// stripped.
    fn type_list(&self, mut i: usize, stop: &[&str]) -> (Vec<String>, usize) {
        let mut out = Vec::new();
        while let Some(t) = self.tok(i) {
            if stop.iter().any(|s| t.is(s)) {
                break;
            }
            if t.is(",") {
                i += 1;
                continue;
            }
            if t.is("@") {
                let (_, next) = self.qualified_name(i + 1);
                i = if self.is(next, "(") {
                    self.m[next].map_or(next + 1, |c| c + 1)
                } else {
                    next
                };
                continue;
            }
            if t.is_ident() {
                let (name, next) = self.qualified_name(i);
                out.push(name);
                i = if self.is(next, "<") {
                    self.skip_angles(next)
                } else {
                    next
                };
                continue;
            }
            i += 1;
        }
        (out, i)
    }

    /// A type declaration whose keyword is at `kw`; returns the index after its closing `}`.
    fn type_decl(
        &mut self,
        kw: usize,
        kind: TypeKind,
        prefix: Prefix,
        parent: Option<usize>,
    ) -> usize {
        let name_at = if kind == TypeKind::Annotation {
            kw + 2
        } else {
            kw + 1
        };
        let Some(name_tok) = self.tok(name_at).filter(|t| t.is_ident()) else {
            self.unclassified += 1;
            return self.skip_member(kw + 1);
        };
        let name = name_tok.text.clone();
        let qualified = match parent {
            Some(p) => format!("{}.{name}", self.types[p].qualified),
            None => match &self.package {
                Some(pkg) => format!("{pkg}.{name}"),
                None => name.clone(),
            },
        };
        let mut i = name_at + 1;
        if self.is(i, "<") {
            i = self.skip_angles(i);
        }
        let mut record_components = Vec::new();
        if kind == TypeKind::Record && self.is(i, "(") {
            let close = self.m[i].unwrap_or(i);
            record_components = self.params(i + 1, close);
            i = close + 1;
        }
        let mut extends = Vec::new();
        let mut implements = Vec::new();
        while let Some(t) = self.tok(i) {
            if t.is("{") {
                break;
            }
            match t.text.as_str() {
                "extends" => {
                    let (l, n) = self.type_list(i + 1, &["{", "implements", "permits"]);
                    extends = l;
                    i = n;
                }
                "implements" => {
                    let (l, n) = self.type_list(i + 1, &["{", "extends", "permits"]);
                    implements = l;
                    i = n;
                }
                "permits" => {
                    let (_, n) = self.type_list(i + 1, &["{", "extends", "implements"]);
                    i = n;
                }
                _ if t.is(";") || t.is("}") => break,
                _ => i += 1,
            }
        }
        if !self.is(i, "{") {
            self.unclassified += 1;
            return i;
        }
        let open = i;
        let close = self.m[open].unwrap_or(self.t.len() - 1);
        let idx = self.types.len();
        let fields = record_components
            .into_iter()
            .map(|p| FieldDecl {
                name: p.name,
                ty: p.ty,
                modifiers: vec!["private".into(), "final".into()],
                doc: None,
                line: self.t[name_at].line,
                init: None,
            })
            .collect();
        self.types.push(TypeDecl {
            name,
            qualified,
            kind,
            modifiers: prefix.modifiers,
            annotations: prefix.annotations,
            extends,
            implements,
            doc: prefix.doc,
            start_line: prefix.start_line.unwrap_or(self.t[kw].line),
            end_line: self.t[close].line,
            parent,
            methods: Vec::new(),
            fields,
        });
        self.type_body(idx, open, close);
        close + 1
    }

    fn type_body(&mut self, idx: usize, open: usize, close: usize) {
        let mut i = open + 1;
        if matches!(self.types[idx].kind, TypeKind::Enum) {
            i = self.enum_constants(i, close);
        }
        while i < close {
            if self.is(i, ";") {
                i += 1;
                continue;
            }
            let (prefix, j) = self.prefix(i);
            if j >= close {
                break;
            }
            if self.is(j, "{") {
                // Instance or `static` initializer.
                i = self.m[j].map_or(j + 1, |c| c + 1);
                continue;
            }
            if let Some(kind) = self.type_keyword(j) {
                i = self.type_decl(j, kind, prefix, Some(idx));
                continue;
            }
            i = self.member(idx, j, prefix, close);
        }
    }

    /// Enum constants `A, B(1), C { … };` — skipped; returns the index after the list.
    fn enum_constants(&self, mut i: usize, close: usize) -> usize {
        while i < close {
            if self.is(i, ";") {
                return i + 1;
            }
            if self.is(i, "(") || self.is(i, "{") {
                i = self.m[i].map_or(i + 1, |c| c + 1);
                continue;
            }
            i += 1;
        }
        i
    }

    /// A method, constructor or field whose header starts at `j`; returns the index after it.
    fn member(&mut self, idx: usize, mut j: usize, prefix: Prefix, close: usize) -> usize {
        if self.is(j, "<") {
            j = self.skip_angles(j);
        }
        // Find the first `(`, `=`, `;`, `,` or `{` at header level.
        let mut k = j;
        while k < close {
            let t = &self.t[k];
            if t.is("<") {
                k = self.skip_angles(k);
                continue;
            }
            if t.is("[") {
                k = self.m[k].map_or(k + 1, |c| c + 1);
                continue;
            }
            if t.is("(") || t.is("=") || t.is(";") || t.is(",") || t.is("{") {
                break;
            }
            k += 1;
        }
        let type_name = self.types[idx].name.clone();
        let is_record = self.types[idx].kind == TypeKind::Record;
        if self.is(k, "(") && k > j && self.t[k - 1].is_ident() {
            return self.method(idx, j, k, prefix, &type_name);
        }
        // Compact canonical constructor of a record: `Name { … }`.
        if is_record && self.is(k, "{") && k == j + 1 && self.t[j].text == type_name {
            let body_close = self.m[k].unwrap_or(k);
            let line = prefix.start_line.unwrap_or(self.t[j].line);
            let signature = self.join(j, k);
            self.types[idx].methods.push(MethodDecl {
                name: type_name.clone(),
                kind: MethodKind::Constructor,
                signature,
                modifiers: prefix.modifiers,
                annotations: prefix.annotations,
                return_type: None,
                params: Vec::new(),
                doc: prefix.doc,
                start_line: line,
                end_line: self.t[body_close].line,
                body: Some((k, body_close)),
            });
            return body_close + 1;
        }
        // `T name[] = …` — C-style dims after the declarator name.
        let mut name_end = k;
        while name_end >= j + 3 && self.is(name_end - 1, "]") && self.is(name_end - 2, "[") {
            name_end -= 2;
        }
        if (self.is(k, "=") || self.is(k, ";") || self.is(k, ","))
            && name_end >= j + 2
            && self.t[j].is_ident()
            && self.t[name_end - 1].is_ident()
        {
            return self.fields(idx, j, name_end, k, prefix, close);
        }
        self.unclassified += 1;
        self.skip_member(k.max(j + 1))
    }

    fn method(
        &mut self,
        idx: usize,
        j: usize,
        paren: usize,
        prefix: Prefix,
        type_name: &str,
    ) -> usize {
        let name = self.t[paren - 1].text.clone();
        let return_type = (paren - 1 > j).then(|| self.join(j, paren - 1));
        let kind_ctor = return_type.is_none() && name == type_name;
        let close_paren = self.m[paren].unwrap_or(paren);
        let params = self.params(paren + 1, close_paren);
        let mut i = close_paren + 1;
        while i < self.t.len() && !self.is(i, "{") && !self.is(i, ";") && !self.is(i, "}") {
            if self.is(i, "(") || self.is(i, "[") {
                i = self.m[i].unwrap_or(i);
            }
            i += 1;
        }
        let header = self.join(j, i);
        let signature = if prefix.modifiers.is_empty() {
            header
        } else {
            format!("{} {header}", prefix.modifiers.join(" "))
        };
        let line = prefix.start_line.unwrap_or(self.t[j].line);
        let (kind, body, end, next) = if self.is(i, "{") {
            let c = self.m[i].unwrap_or(i);
            let kind = if kind_ctor {
                MethodKind::Constructor
            } else {
                MethodKind::Method
            };
            (kind, Some((i, c)), self.t[c].line, c + 1)
        } else {
            let end = self.tok(i).map_or(line, |t| t.line);
            let next = if self.is(i, ";") { i + 1 } else { i };
            (MethodKind::Abstract, None, end, next)
        };
        self.types[idx].methods.push(MethodDecl {
            name,
            kind,
            modifiers: prefix.modifiers,
            annotations: prefix.annotations,
            return_type: if kind_ctor { None } else { return_type },
            params,
            signature,
            doc: prefix.doc,
            start_line: line,
            end_line: end,
            body,
        });
        next
    }

    /// Formal parameters between `from` and `to` (exclusive).
    fn params(&self, from: usize, to: usize) -> Vec<Param> {
        let mut out = Vec::new();
        let mut start = from;
        let mut i = from;
        let mut angle = 0i32;
        while i <= to {
            let at_end = i == to;
            if !at_end {
                let t = &self.t[i];
                if t.is("<") {
                    angle += 1;
                } else if t.is(">") {
                    angle -= 1;
                } else if t.is("(") || t.is("[") {
                    i = self.m[i].unwrap_or(i) + 1;
                    continue;
                }
            }
            if at_end || (angle == 0 && self.t[i].is(",")) {
                if let Some(p) = self.param(start, i) {
                    out.push(p);
                }
                start = i + 1;
            }
            i += 1;
        }
        out
    }

    fn param(&self, mut from: usize, to: usize) -> Option<Param> {
        // Drop annotations and `final`.
        loop {
            if self.is(from, "@") {
                let (_, next) = self.qualified_name(from + 1);
                from = if self.is(next, "(") {
                    self.m[next].map_or(next + 1, |c| c + 1)
                } else {
                    next
                };
            } else if self.is(from, "final") {
                from += 1;
            } else {
                break;
            }
        }
        let mut last = to;
        // `int x[]` — trailing dims belong to the type.
        let mut dims = String::new();
        while last >= from + 2 && self.is(last - 1, "]") && self.is(last - 2, "[") {
            dims.push_str("[]");
            last -= 2;
        }
        if last <= from + 1 || !self.t[last - 1].is_ident() {
            return None;
        }
        let name = self.t[last - 1].text.clone();
        let ty = format!("{}{dims}", self.join(from, last - 1));
        Some(Param { ty, name })
    }

    /// The end of a field initialiser starting at `start`: the first `,` or `;` outside brackets.
    fn initializer_end(&self, start: usize, close: usize) -> usize {
        let mut e = start;
        while e < close && !self.is(e, ",") && !self.is(e, ";") {
            if self.is(e, "(") || self.is(e, "[") || self.is(e, "{") {
                e = self.m[e].unwrap_or(e);
            } else if self.is(e, "<") {
                // A generic constructor call `new HashMap<K, V>()` holds a `,`.
                let after = self.skip_angles(e);
                if self.is(after, "(") || self.is(after, "[") || self.is(after, "{") {
                    e = after - 1;
                }
            }
            e += 1;
        }
        e
    }

    /// `T a = 1, b;` — one [`FieldDecl`] per declarator. `k` is the first `=`, `;` or `,`, and
    /// `name_end` the index after the first declarator's name (before any C-style `[]`).
    fn fields(
        &mut self,
        idx: usize,
        j: usize,
        name_end: usize,
        k: usize,
        prefix: Prefix,
        close: usize,
    ) -> usize {
        let dims = "[]".repeat((k - name_end) / 2);
        let ty = format!("{}{dims}", self.join(j, name_end - 1));
        let line = self.t[name_end - 1].line;
        let mut name = self.t[name_end - 1].text.clone();
        let mut i = k;
        let mut modifiers = prefix.modifiers;
        if self.types[idx].kind == TypeKind::Interface
            || self.types[idx].kind == TypeKind::Annotation
        {
            for implied in ["public", "static", "final"] {
                if !modifiers.iter().any(|m| m == implied) {
                    modifiers.push(implied.into());
                }
            }
        }
        let mut doc = prefix.doc;
        loop {
            let mut init = None;
            if self.is(i, "=") {
                let e = self.initializer_end(i + 1, close);
                init = Some((i + 1, e));
                i = e;
            }
            self.types[idx].fields.push(FieldDecl {
                name: name.clone(),
                ty: ty.clone(),
                modifiers: modifiers.clone(),
                doc: doc.take(),
                line,
                init,
            });
            if self.is(i, ",") && self.tok(i + 1).is_some_and(Token::is_ident) {
                name = self.t[i + 1].text.clone();
                i += 2;
                while self.is(i, "[") {
                    i = self.m[i].map_or(i + 1, |c| c + 1);
                }
                continue;
            }
            break;
        }
        if self.is(i, ";") {
            i + 1
        } else {
            self.skip_member(i)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only_type(src: &str) -> TypeDecl {
        let f = parse(src);
        assert_eq!(f.status, FileStatus::Parsed, "{src}");
        assert_eq!(f.types.len(), 1);
        f.types.into_iter().next().unwrap()
    }

    #[test]
    fn braces_inside_literals_and_comments_do_not_unbalance_the_file() {
        let src = r#"
package p;
class A {
    // a { brace in a comment
    /* and } here */
    String s = "{ \" }";
    char c = '{';
    String t = """
        text { block "with" quotes
        """;
    void m() { if (true) { s = "}"; } }
}
"#;
        let t = only_type(src);
        assert_eq!(t.fields.len(), 3);
        assert_eq!(t.methods.len(), 1);
        assert_eq!(t.methods[0].start_line, 11);
        assert_eq!(t.methods[0].end_line, 11);
        assert_eq!(t.fields[0].name, "s");
    }

    #[test]
    fn package_imports_and_type_header_are_read() {
        let src = "package org.compiere.model;\nimport java.util.List;\nimport static java.lang.Math.*;\nimport org.compiere.util.*;\n\
            public class MInvoice extends X_C_Invoice implements DocAction, IDocsPostProcess {}\n";
        let f = parse(src);
        assert_eq!(f.package.as_deref(), Some("org.compiere.model"));
        assert_eq!(f.imports.len(), 3);
        assert!(f.imports[1].is_static && f.imports[1].wildcard);
        assert!(!f.imports[0].wildcard);
        let t = &f.types[0];
        assert_eq!(t.qualified, "org.compiere.model.MInvoice");
        assert_eq!(t.extends, vec!["X_C_Invoice"]);
        assert_eq!(t.implements, vec!["DocAction", "IDocsPostProcess"]);
    }

    #[test]
    fn generics_and_shift_like_closers_do_not_confuse_headers() {
        let src = "class A<T extends Comparable<T>> extends B<Map<String, List<T>>> implements C<D> {\n\
            public <K> Map<K, List<Map<String,Integer>>> m(Map<K, List<String>> a, int... rest) throws E { return null; }\n}\n";
        let t = only_type(src);
        assert_eq!(t.extends, vec!["B"]);
        assert_eq!(t.implements, vec!["C"]);
        let m = &t.methods[0];
        assert_eq!(m.name, "m");
        assert_eq!(m.params.len(), 2);
        assert_eq!(m.params[0].ty, "Map<K, List<String>>");
        assert_eq!(m.params[1].ty, "int...");
        assert_eq!(m.params[1].name, "rest");
        assert_eq!(m.kind, MethodKind::Method);
    }

    #[test]
    fn constructors_abstract_methods_and_fields_are_told_apart() {
        let src = "public abstract class X_C_Invoice extends PO {\n\
            private static final long serialVersionUID = 20260309L;\n\
            public static final String COLUMNNAME_GrandTotal = \"GrandTotal\";\n\
            int a = 1, b, c[] = {1, 2};\n\
            protected Checkbox m_chk[]=null;\n\
            Map<String, Integer> m = new HashMap<String, Integer>();\n\
            public X_C_Invoice (Properties ctx, int C_Invoice_ID, String trxName) { super(ctx); }\n\
            protected abstract int get_AccessLevel();\n\
            static { init(); }\n\
            { other(); }\n\
        }\n";
        let t = only_type(src);
        let names: Vec<&str> = t.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "serialVersionUID",
                "COLUMNNAME_GrandTotal",
                "a",
                "b",
                "c",
                "m_chk",
                "m"
            ]
        );
        assert_eq!(t.methods.len(), 2);
        assert_eq!(t.methods[0].kind, MethodKind::Constructor);
        assert_eq!(t.methods[0].key(), "X_C_Invoice(Properties,int,String)");
        assert_eq!(t.methods[1].kind, MethodKind::Abstract);
        assert_eq!(t.methods[1].return_type.as_deref(), Some("int"));
    }

    #[test]
    fn nested_types_enums_records_and_annotation_types() {
        let src = "package p;\npublic class Outer {\n\
            enum Status { DRAFT(\"DR\") { void x() {} }, DONE(\"CO\"); private final String code; Status(String c) { code = c; } }\n\
            record Pair(String a, int b) { Pair { check(); } int sum() { return b; } }\n\
            @interface Marker { String value() default \"x\"; int n = 1; }\n\
            interface Callback { void call(); default void twice() { call(); call(); } }\n\
            private static class Inner extends Object { }\n\
        }\n";
        let f = parse(src);
        assert_eq!(f.status, FileStatus::Parsed);
        let names: Vec<&str> = f.types.iter().map(|t| t.qualified.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "p.Outer",
                "p.Outer.Status",
                "p.Outer.Pair",
                "p.Outer.Marker",
                "p.Outer.Callback",
                "p.Outer.Inner"
            ]
        );
        let status = &f.types[1];
        assert_eq!(status.kind, TypeKind::Enum);
        assert_eq!(status.fields.len(), 1);
        assert_eq!(status.methods[0].kind, MethodKind::Constructor);
        let pair = &f.types[2];
        assert_eq!(pair.fields.len(), 2);
        assert_eq!(pair.methods.len(), 2);
        assert_eq!(pair.methods[0].kind, MethodKind::Constructor);
        let marker = &f.types[3];
        assert_eq!(marker.kind, TypeKind::Annotation);
        assert_eq!(marker.methods[0].kind, MethodKind::Abstract);
        assert!(marker.fields[0].modifiers.contains(&"static".to_string()));
        let cb = &f.types[4];
        assert_eq!(cb.methods[1].kind, MethodKind::Method);
        assert!(cb.methods[1].modifiers.contains(&"default".to_string()));
        assert_eq!(f.types[5].parent, Some(0));
    }

    #[test]
    fn anonymous_classes_and_lambdas_stay_inside_their_method() {
        let src = "class A {\n\
            Runnable r = new Runnable() { public void run() { go(); } };\n\
            void m() { list.forEach(x -> { use(x); }); new Thread() { public void run() {} }.start(); }\n\
            void n() {}\n\
        }\n";
        let t = only_type(src);
        let names: Vec<&str> = t.methods.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["m", "n"]);
        assert_eq!(t.fields[0].name, "r");
    }

    #[test]
    fn annotations_and_javadoc_attach_to_their_declaration() {
        let src = "/** Generated Model for C_Invoice */\n@org.adempiere.base.Model(table=\"C_Invoice\")\npublic class X {\n\
            /** Set Grand Total.\n\t\t@param GrandTotal Total amount of document\n\t*/\n\
            @Deprecated(since=\"13\") // use better methods\n\
            public void setGrandTotal (BigDecimal GrandTotal) { set_ValueNoCheck (COLUMNNAME_GrandTotal, GrandTotal); }\n}\n";
        let t = only_type(src);
        assert_eq!(t.annotations[0].simple_name(), "Model");
        assert_eq!(
            t.annotations[0].string_arg("table").as_deref(),
            Some("C_Invoice")
        );
        assert_eq!(t.doc.as_deref(), Some("Generated Model for C_Invoice"));
        assert_eq!(t.start_line, 2);
        let m = &t.methods[0];
        assert_eq!(m.annotations[0].name, "Deprecated");
        let doc = m.doc.as_deref().unwrap();
        assert_eq!(doc_description(doc), "Set Grand Total.");
        assert_eq!(
            doc_params(doc),
            vec![(
                "GrandTotal".to_string(),
                "Total amount of document".to_string()
            )]
        );
        assert_eq!(
            m.signature,
            "public void setGrandTotal(BigDecimal GrandTotal)"
        );
    }

    #[test]
    fn unbalanced_braces_skip_the_file_without_guessing() {
        let f = parse("class A { void m() { }\n");
        assert_eq!(f.status, FileStatus::Skipped);
        assert!(f.types.is_empty());
    }

    #[test]
    fn package_info_is_parsed_with_no_types() {
        let f = parse("/** docs */\n@Deprecated\npackage org.x;\n");
        assert!(f.types.is_empty());
        assert_eq!(f.package.as_deref(), Some("org.x"));
        assert_eq!(f.status, FileStatus::Parsed);
    }

    #[test]
    fn a_member_header_that_is_not_understood_is_counted_not_dropped_silently() {
        let src = "class A { void ok() {} ??? weird; int x; }";
        let f = parse(src);
        assert_eq!(f.status, FileStatus::Partial);
        assert_eq!(f.unclassified_members, 1);
        assert_eq!(f.types[0].methods.len(), 1);
    }

    /// Corpus check (RFC 0209 §10): `EKOS_JAVA_CORPUS=<dir> cargo test -p ekos-recovery
    /// java_corpus -- --ignored --nocapture` parses every `.java` under `<dir>` and prints files
    /// by status, the unclassified member count, and the worst files.
    #[test]
    #[ignore = "needs EKOS_JAVA_CORPUS"]
    fn java_corpus() {
        let Ok(root) = std::env::var("EKOS_JAVA_CORPUS") else {
            return;
        };
        let mut by_status: std::collections::BTreeMap<&str, usize> = Default::default();
        let (mut types, mut methods, mut fields, mut unclassified) = (0, 0, 0, 0);
        let mut worst: Vec<(usize, String)> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        for entry in walkdir::WalkDir::new(&root).into_iter().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("java") {
                continue;
            }
            let bytes = std::fs::read(path).unwrap();
            let f = parse(&String::from_utf8_lossy(&bytes));
            *by_status.entry(f.status.as_str()).or_default() += 1;
            types += f.types.len();
            methods += f.types.iter().map(|t| t.methods.len()).sum::<usize>();
            fields += f.types.iter().map(|t| t.fields.len()).sum::<usize>();
            unclassified += f.unclassified_members;
            if f.unclassified_members > 0 {
                worst.push((f.unclassified_members, path.display().to_string()));
            }
            if f.status == FileStatus::Skipped {
                skipped.push(path.display().to_string());
            }
        }
        worst.sort_by(|a, b| b.cmp(a));
        println!("files by status: {by_status:?}");
        println!(
            "types {types}, methods {methods}, fields {fields}, unclassified members {unclassified}"
        );
        for (n, p) in worst.iter().take(15) {
            println!("  {n:>4} {p}");
        }
        for p in skipped.iter().take(15) {
            println!("  skipped {p}");
        }
    }
}
