//! Regex portability: can other engines run a rule pattern as written, and
//! its ECMAScript form for the release bundle ([`crate::export`]).
//!
//! The bundle carries every pattern in two dialects. `pattern` is the rule
//! text with `(?P<name>)` groups, for Python `re`, Go `regexp` (RE2) and
//! PCRE. The JS form ([`to_js`]) has `(?<name>)` groups and runs with the
//! [`JS_FLAGS`] flags; .NET reads the same form. A construct that cannot be
//! written in both dialects breaks the bundle ([`Issue::breaks_export`]);
//! one only Go RE2 rejects is reported and does not fail.
//!
//! The JS form keeps Rust's meaning where ECMAScript's differs: `\d`, `\w`
//! and `\b` become their Unicode forms (ECMAScript's are ASCII) and `.`
//! becomes `[^\n]` (ECMAScript's `.` also stops at `\r`, U+2028, U+2029).
//!
//! Look-around, backreferences and possessive or atomic groups never reach
//! the AST: the `regex` crate rejects them, so [`issues`] names them from the
//! pattern text instead.

use regex_syntax::ast::{
    self, parse::Parser, AssertionKind, Ast, ClassSetBinaryOp, ClassSetItem, GroupKind,
    HexLiteralKind, LiteralKind, RepetitionRange, SpecialLiteralKind,
};

/// Flags the [`to_js`] form is written for (`new RegExp(js, "u")`).
pub const JS_FLAGS: &str = "u";

/// The members of Rust's Unicode `\w`, written for ECMAScript `u` mode and
/// .NET (Rust's is `Alphabetic`, `M`, `Nd`, `Pc` and `Join_Control`; `L` and
/// `Nl` stand in for `Alphabetic`, which .NET lacks).
const JS_WORD: &str = r"\p{L}\p{Nl}\p{M}\p{Nd}\p{Pc}";

/// The largest repetition count Go RE2 accepts (`{n}`, `{n,m}`).
pub const RE2_MAX_REPEAT: u32 = 1000;

/// One portability problem in a pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// True when the construct cannot be written in both bundle dialects;
    /// false when only Go RE2 rejects it (reported, not a failure).
    pub breaks_export: bool,
    /// What is not portable and to which engines.
    pub what: String,
}

impl Issue {
    fn export(what: impl Into<String>) -> Issue {
        Issue {
            breaks_export: true,
            what: what.into(),
        }
    }
}

/// Characters ECMAScript accepts after `\` in `u` mode (`-` only in a class).
fn js_escapable(c: char, in_class: bool) -> bool {
    "^$\\.*+?()[]{}|/".contains(c) || (in_class && c == '-')
}

/// Names the constructs the `regex` crate refuses to parse, by a scan of the
/// pattern text that skips escapes and bracketed classes.
fn unsupported(pattern: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    let mut add = |what| {
        if !found.contains(&what) {
            found.push(what);
        }
    };
    let chars: Vec<char> = pattern.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let mut in_class = false;
    let mut i = 0;
    while let Some(c) = at(i) {
        match c {
            '\\' => {
                match at(i + 1) {
                    Some('1'..='9') if !in_class => add("backreference"),
                    Some('k') if matches!(at(i + 2), Some('<' | '{' | '\'')) => {
                        add("backreference");
                    }
                    _ => {}
                }
                i += 1;
            }
            '[' => in_class = true,
            ']' => in_class = false,
            _ if in_class => {}
            '(' if at(i + 1) == Some('?') => match (at(i + 2), at(i + 3)) {
                (Some('<'), Some('=' | '!')) => add("lookbehind"),
                (Some('=' | '!'), _) => add("lookahead"),
                (Some('>'), _) => add("atomic group"),
                (Some('P'), Some('=')) => add("backreference"),
                _ => {}
            },
            '*' | '+' | '?' | '}' if at(i + 1) == Some('+') => add("possessive quantifier"),
            _ => {}
        }
        i += 1;
    }
    found
}

/// Why the `regex` crate cannot load `pattern`, in portability terms, or
/// `None` when it parses (or fails for another reason).
pub fn unsupported_hint(pattern: &str) -> Option<String> {
    let found = unsupported(pattern);
    (!found.is_empty()).then(|| {
        format!(
            "{} (not portable: Go RE2 and Rust regex have none)",
            found.join(", ")
        )
    })
}

/// Collects issues and the edits that turn a pattern into its JS form.
#[derive(Default)]
struct Walk {
    issues: Vec<Issue>,
    /// `(start, end, replacement)` byte edits, in visit order.
    edits: Vec<(usize, usize, String)>,
}

impl Walk {
    fn literal(&mut self, lit: &ast::Literal, in_class: bool) {
        let (start, end) = (lit.span.start.offset, lit.span.end.offset);
        match &lit.kind {
            LiteralKind::Verbatim => {
                // Lone `]`, `{` and `}` are literal in Rust but syntax errors in
                // ECMAScript `u` mode; a leading `]` in a class would end it.
                let special = if in_class { "]" } else { "]{}" };
                if special.contains(lit.c) {
                    self.edits.push((start, end, format!("\\{}", lit.c)));
                }
            }
            LiteralKind::Meta | LiteralKind::Superfluous => {
                if !js_escapable(lit.c, in_class) {
                    self.edits.push((start, end, lit.c.to_string()));
                }
            }
            LiteralKind::HexFixed(HexLiteralKind::X) => {}
            LiteralKind::HexFixed(_) | LiteralKind::HexBrace(_) | LiteralKind::Octal => self
                .issues
                .push(Issue::export(format!(
                    "{:?} escape for {:?}: write \\xHH or the character itself (Python re, PCRE and ECMAScript disagree)",
                    lit.kind, lit.c
                ))),
            LiteralKind::Special(SpecialLiteralKind::Bell | SpecialLiteralKind::Space) => {
                self.issues.push(Issue::export(
                    "\\a or an escaped space: ECMAScript has neither",
                ));
            }
            LiteralKind::Special(_) => {}
        }
    }

    fn perl(&mut self, class: &ast::ClassPerl, in_class: bool) {
        let (start, end) = (class.span.start.offset, class.span.end.offset);
        let with = match (&class.kind, class.negated, in_class) {
            (ast::ClassPerlKind::Space, ..) => return,
            (ast::ClassPerlKind::Digit, false, _) => r"\p{Nd}".to_string(),
            (ast::ClassPerlKind::Digit, true, _) => r"\P{Nd}".to_string(),
            (ast::ClassPerlKind::Word, false, false) => format!("[{JS_WORD}]"),
            (ast::ClassPerlKind::Word, true, false) => format!("[^{JS_WORD}]"),
            (ast::ClassPerlKind::Word, false, true) => JS_WORD.to_string(),
            (ast::ClassPerlKind::Word, true, true) => {
                self.issues.push(Issue::export(
                    "\\W inside a class: ECMAScript cannot write Unicode \\W there; use [^\\w..]",
                ));
                return;
            }
        };
        self.edits.push((start, end, with));
    }

    fn unicode_class(&mut self) {
        self.issues
            .push(Issue::export("\\p{..} Unicode class: Python re has none"));
    }
}

impl ast::Visitor for Walk {
    type Output = Walk;
    type Err = std::convert::Infallible;

    fn finish(self) -> Result<Walk, Self::Err> {
        Ok(self)
    }

    fn visit_pre(&mut self, ast: &Ast) -> Result<(), Self::Err> {
        match ast {
            Ast::Flags(f) => self.issues.push(Issue::export(format!(
                "inline flags (?{}): ECMAScript has none",
                flags_text(&f.flags)
            ))),
            Ast::Literal(lit) => self.literal(lit, false),
            Ast::Assertion(a) => match a.kind {
                AssertionKind::StartLine | AssertionKind::EndLine => {}
                AssertionKind::WordBoundary => self.edits.push((
                    a.span.start.offset,
                    a.span.end.offset,
                    format!("(?:(?<=[{JS_WORD}])(?![{JS_WORD}])|(?<![{JS_WORD}])(?=[{JS_WORD}]))"),
                )),
                AssertionKind::NotWordBoundary => self.edits.push((
                    a.span.start.offset,
                    a.span.end.offset,
                    format!("(?:(?<=[{JS_WORD}])(?=[{JS_WORD}])|(?<![{JS_WORD}])(?![{JS_WORD}]))"),
                )),
                AssertionKind::StartText | AssertionKind::EndText => self.issues.push(
                    Issue::export("\\A or \\z: ECMAScript has neither; use ^ and $"),
                ),
                _ => self.issues.push(Issue::export(
                    "\\b{..}, \\< or \\> word boundary: only Rust regex has it",
                )),
            },
            Ast::ClassUnicode(_) => self.unicode_class(),
            Ast::ClassPerl(class) => self.perl(class, false),
            Ast::Dot(span) => {
                self.edits
                    .push((span.start.offset, span.end.offset, r"[^\n]".into()))
            }
            Ast::Repetition(r) => {
                let max = match r.op.kind {
                    ast::RepetitionKind::Range(
                        RepetitionRange::Exactly(n)
                        | RepetitionRange::AtLeast(n)
                        | RepetitionRange::Bounded(_, n),
                    ) => n,
                    _ => 0,
                };
                if max > RE2_MAX_REPEAT {
                    self.issues.push(Issue {
                        breaks_export: false,
                        what: format!(
                            "repetition count {max} > {RE2_MAX_REPEAT}: Go RE2 rejects it"
                        ),
                    });
                }
            }
            Ast::Group(g) => match &g.kind {
                GroupKind::CaptureName {
                    starts_with_p,
                    name,
                } => {
                    if !starts_with_p {
                        self.issues.push(Issue::export(format!(
                            "(?<{}>: write (?P<{}> (Python re needs it; the JS form is derived)",
                            name.name, name.name
                        )));
                    } else {
                        let at = g.span.start.offset;
                        self.edits.push((at, at + 3, "(?".into()));
                    }
                    if !name
                        .name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        self.issues.push(Issue::export(format!(
                            "group name {:?}: Python re and ECMAScript need an identifier",
                            name.name
                        )));
                    }
                }
                GroupKind::NonCapturing(flags) if !flags.items.is_empty() => {
                    self.issues.push(Issue::export(format!(
                        "inline flags (?{}:..): ECMAScript has none",
                        flags_text(flags)
                    )));
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    fn visit_class_set_item_pre(&mut self, item: &ClassSetItem) -> Result<(), Self::Err> {
        match item {
            ClassSetItem::Literal(lit) => self.literal(lit, true),
            ClassSetItem::Range(r) => {
                self.literal(&r.start, true);
                self.literal(&r.end, true);
            }
            ClassSetItem::Ascii(_) => self.issues.push(Issue::export(
                "[[:name:]] ASCII class: Python re and ECMAScript have none",
            )),
            ClassSetItem::Unicode(_) => self.unicode_class(),
            ClassSetItem::Perl(class) => self.perl(class, true),
            ClassSetItem::Bracketed(_) => self.issues.push(Issue::export(
                "nested character class: only Rust regex has it",
            )),
            _ => {}
        }
        Ok(())
    }

    fn visit_class_set_binary_op_pre(&mut self, _op: &ClassSetBinaryOp) -> Result<(), Self::Err> {
        self.issues.push(Issue::export(
            "character class set operation (&&, --, ~~): only Rust regex has it",
        ));
        Ok(())
    }
}

fn flags_text(flags: &ast::Flags) -> String {
    flags
        .items
        .iter()
        .map(|item| match item.kind {
            ast::FlagsItemKind::Negation => '-',
            ast::FlagsItemKind::Flag(f) => match f {
                ast::Flag::CaseInsensitive => 'i',
                ast::Flag::MultiLine => 'm',
                ast::Flag::DotMatchesNewLine => 's',
                ast::Flag::SwapGreed => 'U',
                ast::Flag::Unicode => 'u',
                ast::Flag::CRLF => 'R',
                ast::Flag::IgnoreWhitespace => 'x',
            },
        })
        .collect()
}

fn walk(pattern: &str) -> Result<Walk, Vec<Issue>> {
    let parsed = Parser::new().parse(pattern).map_err(|e| {
        vec![Issue::export(
            unsupported_hint(pattern).unwrap_or_else(|| format!("does not parse: {}", e.kind())),
        )]
    })?;
    let Ok(w) = ast::visit(&parsed, Walk::default());
    Ok(w)
}

/// Every portability issue in `pattern` (empty when it runs everywhere).
pub fn issues(pattern: &str) -> Vec<Issue> {
    walk(pattern).map_or_else(|e| e, |w| w.issues)
}

/// The ECMAScript (and .NET) form of `pattern`, to run with [`JS_FLAGS`]:
/// `(?P<name>` becomes `(?<name>`, escapes `u` mode rejects lose their
/// backslash, lone `]`, `{`, `}` gain one, and `\d`, `\w`, `\b`, `.` keep
/// their Rust meaning (see the module docs). `None` when the pattern has an
/// issue that [`Issue::breaks_export`].
pub fn to_js(pattern: &str) -> Option<String> {
    let mut w = walk(pattern).ok()?;
    if w.issues.iter().any(|i| i.breaks_export) {
        return None;
    }
    w.edits.sort_by_key(|e| e.0);
    let mut out = String::with_capacity(pattern.len());
    let mut at = 0;
    for (start, end, with) in w.edits {
        out.push_str(pattern.get(at..start)?);
        out.push_str(&with);
        at = end;
    }
    out.push_str(pattern.get(at..)?);
    Some(out)
}
