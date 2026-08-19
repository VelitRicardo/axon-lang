//! v1.30.0 — Parameterized `where`-expression filter
//! compiler for the `axonstore` cognitive data plane.
//!
//! # D4 — SQL-injection-proof by construction
//!
//! A `retrieve from S where "<expr>"` step carries `where_expr` as a
//! raw string. This module compiles that string into a parameterized
//! Postgres `WHERE` clause: the SQL **structure** (column identifiers,
//! operators, connectors) and the user **values** are separated, and
//! every value renders as a `$N` bind placeholder. **No code path in
//! this module interpolates a user value into SQL text** — that is the
//! load-bearing D4 invariant, fuzzed in 35.k.
//!
//! ```text
//!   where "id = 1 AND name = 'Alice'"          (column types unknown)
//!     →  ("\"id\"::text = $1 AND \"name\"::text = $2",
//!         [Integer(1), Text("Alice")])
//! ```
//!
//! # Grammar (closed)
//!
//! ```text
//!   filter     := condition (connector condition)*
//!   condition  := column operator value
//!   column     := [A-Za-z_][A-Za-z0-9_]*           (ASCII; ≤ 63 bytes)
//!   operator   := '=' | '==' | '!=' | '<>' | '>' | '>=' | '<' | '<=' | LIKE
//!   connector  := AND | OR                          (case-insensitive)
//!   value      := string-literal | number | TRUE | FALSE | NULL
//! ```
//!
//! Operator precedence is **SQL's native precedence** (`AND` binds
//! tighter than `OR`) — the flat condition list renders verbatim and
//! Postgres applies precedence. Parenthesized grouping is a documented
//! future extension; v1.30.0's `where` grammar is the flat form.
//!
//! # Why this is stricter than the Python reference
//!
//! The frozen Python `filter_parser.py` has a real defect: a trailing
//! connector (`"id = 1 AND"`) parses to one condition + one connector
//! and renders the broken SQL `"id" = $1 AND`. This compiler rejects a
//! dangling connector as a typed [`FilterError`]. It is **total**:
//! every input either compiles to a parameterized clause or returns a
//! named error — never a panic, never broken SQL.

use std::fmt;

/// Postgres `NAMEDATALEN - 1` — the maximum identifier byte length.
/// A longer column name cannot name a real Postgres column, so it is
/// rejected here with a clear error rather than deferred to a cryptic
/// runtime SQL failure.
const MAX_COLUMN_LEN: usize = 63;

/// Upper bound on conditions in a single `where` expression. A real
/// agent store query never approaches this; the cap is a denial-of-
/// service guard (an adversarial `a=1 AND a=1 AND …` cannot allocate
/// an unbounded parameter vector) and stays far inside Postgres' own
/// 65535-bind-parameter ceiling.
const MAX_CONDITIONS: usize = 256;

/// `true` iff `name` is a safe SQL identifier for the `axonstore` data
/// plane: ASCII `[A-Za-z_][A-Za-z0-9_]*`, 1..=63 bytes.
///
/// The `where`-clause lexer enforces this discipline structurally for
/// filter columns. Table names and `insert`/`mutate` column names —
/// which the 35.c backend quotes directly into SQL text — are checked
/// against this predicate before quoting, so that **no untrusted
/// identifier ever reaches SQL** (the D4 invariant applied to the
/// identifier surface, not just the value surface).
pub fn is_safe_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_COLUMN_LEN
        && name.bytes().enumerate().all(|(i, b)| {
            b == b'_'
                || b.is_ascii_alphabetic()
                || (i > 0 && b.is_ascii_digit())
        })
}

// ════════════════════════════════════════════════════════════════════
//  Value catalog
// ════════════════════════════════════════════════════════════════════

/// A typed filter value — the closed catalog of things that can appear
/// in value position. Every non-[`SqlValue::Null`] value is rendered as
/// a `$N` bind placeholder; `Null` is folded into `IS NULL` /
/// `IS NOT NULL` and so never occupies a parameter slot.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    /// A string literal (quote characters stripped, escapes resolved).
    Text(String),
    /// An integer literal that fits in `i64`.
    Integer(i64),
    /// A finite floating-point literal.
    Float(f64),
    /// A boolean literal (`true` / `false`, case-insensitive).
    Boolean(bool),
    /// The SQL `NULL` literal.
    Null,
}

impl SqlValue {
    /// A human-readable type name, used in error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            SqlValue::Text(_) => "text",
            SqlValue::Integer(_) => "integer",
            SqlValue::Float(_) => "float",
            SqlValue::Boolean(_) => "boolean",
            SqlValue::Null => "null",
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  Operator catalog (closed, whitelisted)
// ════════════════════════════════════════════════════════════════════

/// A comparison operator. The whitelist IS the catalog — there is no
/// path by which an un-listed operator reaches the rendered SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    /// `=` / `==`
    Eq,
    /// `!=` / `<>`
    Ne,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `LIKE` (case-insensitive surface spelling)
    Like,
}

impl Operator {
    /// The canonical SQL spelling of this operator.
    pub fn as_sql(self) -> &'static str {
        match self {
            Operator::Eq => "=",
            Operator::Ne => "!=",
            Operator::Gt => ">",
            Operator::Ge => ">=",
            Operator::Lt => "<",
            Operator::Le => "<=",
            Operator::Like => "LIKE",
        }
    }

    /// Resolve a comparison *symbol* (not `LIKE`) to its operator.
    /// `==` normalizes to `=`; `<>` normalizes to `!=`.
    fn from_symbol(sym: &str) -> Option<Operator> {
        Some(match sym {
            "=" | "==" => Operator::Eq,
            "!=" | "<>" => Operator::Ne,
            ">" => Operator::Gt,
            ">=" => Operator::Ge,
            "<" => Operator::Lt,
            "<=" => Operator::Le,
            _ => return None,
        })
    }

    /// `true` iff this operator is meaningful against `NULL`. Only
    /// equality (`=` → `IS NULL`) and inequality (`!=` → `IS NOT NULL`)
    /// are; an ordering or `LIKE` comparison with `NULL` is always
    /// `UNKNOWN` in SQL and is therefore a user error, rejected here.
    fn accepts_null(self) -> bool {
        matches!(self, Operator::Eq | Operator::Ne)
    }
}

impl fmt::Display for Operator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_sql())
    }
}

// ════════════════════════════════════════════════════════════════════
//  Connector catalog (closed)
// ════════════════════════════════════════════════════════════════════

/// A boolean connector joining two adjacent conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connector {
    /// `AND`
    And,
    /// `OR`
    Or,
}

impl Connector {
    /// The canonical SQL spelling of this connector.
    pub fn as_sql(self) -> &'static str {
        match self {
            Connector::And => "AND",
            Connector::Or => "OR",
        }
    }
}

impl fmt::Display for Connector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_sql())
    }
}

// ════════════════════════════════════════════════════════════════════
// Time-relative value (v2.21.0 — closed, structural)
// ════════════════════════════════════════════════════════════════════

/// A closed catalog of interval units. The whitelist IS the catalog — an
/// un-listed unit cannot reach the rendered SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUnit {
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl TimeUnit {
    /// Resolve a unit word (singular or plural, case-insensitive) to a unit.
    fn from_word(w: &str) -> Option<TimeUnit> {
        Some(match w.to_ascii_lowercase().as_str() {
            "second" | "seconds" => TimeUnit::Second,
            "minute" | "minutes" => TimeUnit::Minute,
            "hour" | "hours" => TimeUnit::Hour,
            "day" | "days" => TimeUnit::Day,
            "week" | "weeks" => TimeUnit::Week,
            "month" | "months" => TimeUnit::Month,
            "year" | "years" => TimeUnit::Year,
            _ => return None,
        })
    }

    /// The canonical singular SQL unit word (Postgres `interval` accepts it).
    fn as_sql(self) -> &'static str {
        match self {
            TimeUnit::Second => "second",
            TimeUnit::Minute => "minute",
            TimeUnit::Hour => "hour",
            TimeUnit::Day => "day",
            TimeUnit::Week => "week",
            TimeUnit::Month => "month",
            TimeUnit::Year => "year",
        }
    }
}

/// `+` or `-` in `now() ± interval '…'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeSign {
    Plus,
    Minus,
}

/// A structural time-relative value: `now()`, optionally offset by
/// `± interval '<amount> <unit>'`. v2.21.0.
///
/// **D4 preserved.** This renders DIRECTLY to SQL — but `now()` and `interval`
/// are KEYWORDS, not adopter values, and the only adopter-varying parts are a
/// validated `u32` amount + a closed-catalog [`TimeUnit`], both re-emitted
/// structurally. No adopter string is ever interpolated into SQL text, so the
/// v1.30.0 "no user value reaches SQL" invariant holds. The grammar admits
/// EXACTLY this one function-shaped value (not arbitrary SQL functions) — that
/// is the line that keeps the compiler total + injection-proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeValue {
    /// `None` = bare `now()`. `Some((sign, amount, unit))` = `now() ± interval`.
    pub offset: Option<(TimeSign, u32, TimeUnit)>,
}

impl TimeValue {
    /// Render to a structural SQL expression: `now()` or
    /// `now() - interval '30 minute'`. Injection-free by construction (the
    /// amount is a re-rendered `u32`; the unit is a whitelisted keyword).
    fn to_sql(self) -> String {
        match self.offset {
            None => "now()".to_string(),
            Some((sign, amount, unit)) => {
                let s = match sign {
                    TimeSign::Plus => "+",
                    TimeSign::Minus => "-",
                };
                format!("now() {s} interval '{amount} {}'", unit.as_sql())
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  Filter AST
// ════════════════════════════════════════════════════════════════════

/// The right-hand side of a condition: either a bound literal value (rendered
/// as a `$N` placeholder — the v1.30.0 D4 path) or a structural time-relative
/// value (rendered inline; v2.21.0).
#[derive(Debug, Clone, PartialEq)]
pub enum Rhs {
    /// A literal value — renders as a `$N` bind.
    Value(SqlValue),
    /// `now()` / `now() ± interval '<n> <unit>'` — renders inline as SQL.
    Time(TimeValue),
}

/// A single `column op value` predicate.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterCondition {
    /// The column identifier (validated `[A-Za-z_]\w*`, ≤ 63 bytes).
    pub column: String,
    /// The comparison operator.
    pub op: Operator,
    /// The right-hand side: a bound literal or a v2.21.0 time value.
    pub value: Rhs,
}

/// A parsed `where` expression: a flat list of conditions joined by
/// connectors. The structural invariant — enforced by [`parse_filter`]
/// — is `connectors.len() + 1 == conditions.len()` whenever
/// `conditions` is non-empty (and both empty for an empty expression).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Filter {
    /// The conditions, in source order.
    pub conditions: Vec<FilterCondition>,
    /// `connectors[i]` joins `conditions[i]` and `conditions[i + 1]`.
    pub connectors: Vec<Connector>,
}

impl Filter {
    /// `true` iff this filter carries no conditions (an empty `where`).
    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
    }
}

// ════════════════════════════════════════════════════════════════════
//  Error catalog (typed, total — D7 honest failure surface)
// ════════════════════════════════════════════════════════════════════

/// Every way a `where` expression can fail to compile. The compiler is
/// total: it returns one of these or a clause — never a panic.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterError {
    /// A character outside the grammar's alphabet (e.g. `;`, `(`, `%`).
    UnexpectedChar { ch: char, pos: usize },
    /// A string literal opened but never closed.
    UnterminatedString { pos: usize },
    /// A numeric token that is neither a valid `i64` nor a finite `f64`.
    InvalidNumber { token: String },
    /// A column identifier was expected but a non-identifier was found.
    ExpectedColumn { found: String },
    /// A column identifier exceeds Postgres' 63-byte limit.
    ColumnTooLong { column: String, len: usize },
    /// An operator was expected after a column but something else was found.
    ExpectedOperator { column: String, found: String },
    /// The expression ended where an operator was expected.
    MissingOperator { column: String },
    /// A value was expected after an operator but a symbol was found.
    ExpectedValue { found: String },
    /// The expression ended where a value was expected.
    MissingValue { column: String },
    /// A bare word in value position — string values must be quoted.
    UnquotedValue { token: String },
    /// Two conditions were adjacent with no `AND`/`OR` between them.
    ExpectedConnector { found: String },
    /// A trailing `AND`/`OR` with no following condition (the defect
    /// the frozen Python reference silently mis-renders).
    DanglingConnector { connector: Connector },
    /// `NULL` compared with an operator other than `=` / `!=`.
    NullWithNonEqualityOp { column: String, op: Operator },
    /// `LIKE` applied to a non-text value.
    LikeRequiresText { column: String, found: &'static str },
    /// More than [`MAX_CONDITIONS`] conditions in one expression.
    TooManyConditions { limit: usize },
    /// v2.21.0 — a malformed `now() ± interval '<n> <unit>'` time value:
    /// a bad `now()` shape, a non-`u32` amount, or an unknown unit.
    BadTimeValue { detail: String },
    /// v2.21.0 — `LIKE` applied to a `now()` time value (nonsensical).
    LikeWithTime,
    /// v2.21.0 — a malformed `order_by:` term: an empty term, a column
    /// that fails the identifier discipline, or a direction that is not
    /// `asc`/`desc`.
    BadOrderBy { detail: String },
    /// v2.21.0 — a malformed `limit:`: not a non-negative integer that
    /// fits `u32` (after `${binding}` resolution).
    BadLimit { detail: String },
    /// v2.33.0 — a malformed `aggregate:`: not a member of the closed
    /// catalog (`count` | `sum(col)` | `avg(col)` | `min(col)` | `max(col)`),
    /// a bad column identifier, or an unsupported combination (v1 rejects
    /// `aggregate:` together with `order_by:`/`limit:`).
    BadAggregate { detail: String },
    /// v2.33.0 — a malformed `group_by:`: an invalid column identifier,
    /// an empty term, or a `group_by:` without an `aggregate:`.
    BadGroupBy { detail: String },
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilterError::UnexpectedChar { ch, pos } => write!(
                f,
                "unexpected character {ch:?} at position {pos} — the \
                 where-grammar alphabet is identifiers, comparison \
                 symbols, quoted strings and numbers"
            ),
            FilterError::UnterminatedString { pos } => {
                write!(f, "unterminated string literal opened at position {pos}")
            }
            FilterError::InvalidNumber { token } => write!(
                f,
                "invalid numeric literal `{token}` — expected an integer \
                 or a finite decimal"
            ),
            FilterError::ExpectedColumn { found } => write!(
                f,
                "expected a column name, found `{found}` — a condition \
                 is `column op value`"
            ),
            FilterError::ColumnTooLong { column, len } => write!(
                f,
                "column name `{column}` is {len} bytes — exceeds the \
                 Postgres {MAX_COLUMN_LEN}-byte identifier limit"
            ),
            FilterError::ExpectedOperator { column, found } => write!(
                f,
                "expected a comparison operator after column `{column}`, \
                 found `{found}`"
            ),
            FilterError::MissingOperator { column } => write!(
                f,
                "expected a comparison operator after column `{column}`, \
                 found end of expression"
            ),
            FilterError::ExpectedValue { found } => write!(
                f,
                "expected a value, found `{found}`"
            ),
            FilterError::MissingValue { column } => write!(
                f,
                "expected a value for column `{column}`, found end of \
                 expression"
            ),
            FilterError::UnquotedValue { token } => write!(
                f,
                "unquoted value `{token}` — string values must be quoted \
                 (`'{token}'`); only numbers and `true`/`false`/`null` \
                 are bare"
            ),
            FilterError::ExpectedConnector { found } => write!(
                f,
                "expected `AND` or `OR` between conditions, found `{found}`"
            ),
            FilterError::DanglingConnector { connector } => write!(
                f,
                "expression ends with a dangling `{connector}` — a \
                 connector must be followed by another condition"
            ),
            FilterError::NullWithNonEqualityOp { column, op } => write!(
                f,
                "`null` compared with `{op}` on column `{column}` — \
                 `null` is only valid with `=` (renders `IS NULL`) or \
                 `!=` (renders `IS NOT NULL`)"
            ),
            FilterError::LikeRequiresText { column, found } => write!(
                f,
                "`LIKE` on column `{column}` requires a text value, \
                 found {found}"
            ),
            FilterError::TooManyConditions { limit } => write!(
                f,
                "where expression exceeds the {limit}-condition limit"
            ),
            FilterError::BadTimeValue { detail } => write!(
                f,
                "malformed time value: {detail} — the supported forms are \
                 `now()` and `now() ± interval '<n> <unit>'` where <unit> is \
                 second/minute/hour/day/week/month/year (e.g. \
                 `last_activity_at < now() - interval '30 minutes'`)"
            ),
            FilterError::LikeWithTime => write!(
                f,
                "`LIKE` cannot be applied to a `now()` time value — use an \
                 ordering operator (`<`, `>`, `<=`, `>=`) or `=`/`!=`"
            ),
            FilterError::BadOrderBy { detail } => write!(
                f,
                "malformed `order_by:` — {detail}. The form is a \
                 comma-separated list of `column [asc|desc]` (e.g. \
                 `order_by: \"last_activity_at asc, id desc\"`)"
            ),
            FilterError::BadLimit { detail } => write!(
                f,
                "malformed `limit:` — {detail}. The form is a non-negative \
                 integer literal (`limit: 100`) or a `${{binding}}` that \
                 resolves to one (`limit: \"${{max}}\"`)"
            ),
            FilterError::BadAggregate { detail } => write!(
                f,
                "malformed `aggregate:` — {detail}. The closed catalog is \
                 `count`, `sum(<col>)`, `avg(<col>)`, `min(<col>)`, \
                 `max(<col>)` (e.g. `aggregate: \"count\"`)"
            ),
            FilterError::BadGroupBy { detail } => write!(
                f,
                "malformed `group_by:` — {detail}. The form is a \
                 comma-separated list of column identifiers, and it \
                 requires an `aggregate:` (e.g. `group_by: \"industry\"`)"
            ),
        }
    }
}

impl std::error::Error for FilterError {}

// ════════════════════════════════════════════════════════════════════
//  Tokenizer
// ════════════════════════════════════════════════════════════════════

/// A lexical token. The tokenizer is purely lexical — keyword meaning
/// (`AND`/`OR`/`LIKE`/`TRUE`/`FALSE`/`NULL`) is assigned positionally
/// by the parser.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// An ASCII identifier-shaped word: a column or a keyword.
    Word(String),
    /// A comparison symbol: one of `= == != <> > >= < <=`.
    Symbol(String),
    /// A string literal's content (quotes stripped, escapes resolved).
    Str(String),
    /// A raw numeric token (sign + digits + dots), parsed later.
    Num(String),
}

/// A short human description of a token, for error messages.
fn describe(tok: &Token) -> String {
    match tok {
        Token::Word(w) => w.clone(),
        Token::Symbol(s) => s.clone(),
        Token::Str(s) => format!("'{s}'"),
        Token::Num(n) => n.clone(),
    }
}

/// Lex a `where` expression into tokens. Total: every input either
/// yields a token vector or a [`FilterError`].
fn tokenize(expr: &str) -> Result<Vec<Token>, FilterError> {
    let chars: Vec<char> = expr.chars().collect();
    let n = chars.len();
    let mut tokens: Vec<Token> = Vec::new();
    let mut i = 0;

    while i < n {
        let c = chars[i];

        // — whitespace —
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // — string literal (single- or double-quoted) —
        if c == '\'' || c == '"' {
            let quote = c;
            let mut buf = String::new();
            let mut j = i + 1;
            let mut closed = false;
            while j < n {
                let cj = chars[j];
                if cj == '\\' {
                    // An escape takes the NEXT char literally. A
                    // trailing backslash leaves the string unclosed.
                    if j + 1 < n {
                        buf.push(chars[j + 1]);
                        j += 2;
                        continue;
                    }
                    break;
                }
                if cj == quote {
                    closed = true;
                    j += 1;
                    break;
                }
                buf.push(cj);
                j += 1;
            }
            if !closed {
                return Err(FilterError::UnterminatedString { pos: i });
            }
            tokens.push(Token::Str(buf));
            i = j;
            continue;
        }

        // — comparison symbols —
        if c == '=' || c == '!' || c == '<' || c == '>' {
            if i + 1 < n {
                let two = match (c, chars[i + 1]) {
                    ('=', '=') => Some("=="),
                    ('!', '=') => Some("!="),
                    ('<', '=') => Some("<="),
                    ('>', '=') => Some(">="),
                    ('<', '>') => Some("<>"),
                    _ => None,
                };
                if let Some(sym) = two {
                    tokens.push(Token::Symbol(sym.to_string()));
                    i += 2;
                    continue;
                }
            }
            // `!` is only ever valid as the start of `!=`.
            if c == '!' {
                return Err(FilterError::UnexpectedChar { ch: '!', pos: i });
            }
            tokens.push(Token::Symbol(c.to_string()));
            i += 1;
            continue;
        }

        // — numeric literal (optional leading `-`, digits, dots) —
        if c.is_ascii_digit()
            || (c == '-' && i + 1 < n && chars[i + 1].is_ascii_digit())
        {
            let start = i;
            let mut j = if c == '-' { i + 1 } else { i };
            while j < n && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }
            tokens.push(Token::Num(chars[start..j].iter().collect()));
            i = j;
            continue;
        }

        // v2.21.0: parens + interval sign for the `now() ± interval '…'`
        //   time-value form. `(` `)` `+` and a standalone `-` (a `-` followed
        //   by a digit was already consumed as a negative number above) become
        //   bare symbols the parser interprets ONLY inside the time-value form;
        //   anywhere else they surface as a typed error (totality preserved).
        if c == '(' || c == ')' || c == '+' || c == '-' {
            tokens.push(Token::Symbol(c.to_string()));
            i += 1;
            continue;
        }

        // — identifier / keyword word —
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            let mut j = i;
            while j < n && (chars[j].is_ascii_alphanumeric() || chars[j] == '_')
            {
                j += 1;
            }
            tokens.push(Token::Word(chars[start..j].iter().collect()));
            i = j;
            continue;
        }

        return Err(FilterError::UnexpectedChar { ch: c, pos: i });
    }

    Ok(tokens)
}

/// Parse a numeric token into a typed [`SqlValue`]. Prefers `i64`;
/// falls back to a finite `f64`; otherwise a typed error.
fn parse_number(raw: &str) -> Result<SqlValue, FilterError> {
    if let Ok(n) = raw.parse::<i64>() {
        return Ok(SqlValue::Integer(n));
    }
    if let Ok(x) = raw.parse::<f64>() {
        if x.is_finite() {
            return Ok(SqlValue::Float(x));
        }
    }
    Err(FilterError::InvalidNumber { token: raw.to_string() })
}

/// Resolve a token in *value position* to a typed [`SqlValue`].
fn parse_value(tok: &Token) -> Result<SqlValue, FilterError> {
    match tok {
        Token::Str(s) => Ok(SqlValue::Text(s.clone())),
        Token::Num(raw) => parse_number(raw),
        Token::Word(w) => match w.to_ascii_lowercase().as_str() {
            "true" => Ok(SqlValue::Boolean(true)),
            "false" => Ok(SqlValue::Boolean(false)),
            "null" => Ok(SqlValue::Null),
            _ => Err(FilterError::UnquotedValue { token: w.clone() }),
        },
        Token::Symbol(s) => Err(FilterError::ExpectedValue { found: s.clone() }),
    }
}

/// v2.21.0 — parse the right-hand side starting at `tokens[0]`: either a
/// structural `now() ± interval '<n> <unit>'` time value (which spans several
/// tokens) or a single bound literal. Returns the [`Rhs`] + the number of
/// tokens consumed. The caller guarantees `tokens` is non-empty.
fn parse_rhs(tokens: &[Token], column: &str, op: Operator) -> Result<(Rhs, usize), FilterError> {
    // A `now`-led value is the time form (v2.21.0). `now` is not a valid bare
    // literal otherwise (it would be an `UnquotedValue`), so routing it here
    // does not shadow any pre-67.a behaviour.
    if let Some(Token::Word(w)) = tokens.first() {
        if w.eq_ignore_ascii_case("now") {
            let (tv, consumed) = parse_time_value(tokens, op)?;
            return Ok((Rhs::Time(tv), consumed));
        }
    }

    // — bound literal (the pre-67.a path + its semantic checks) —
    let value = parse_value(&tokens[0])?;
    if matches!(value, SqlValue::Null) && !op.accepts_null() {
        return Err(FilterError::NullWithNonEqualityOp {
            column: column.to_string(),
            op,
        });
    }
    if op == Operator::Like && !matches!(value, SqlValue::Text(_)) {
        return Err(FilterError::LikeRequiresText {
            column: column.to_string(),
            found: value.type_name(),
        });
    }
    Ok((Rhs::Value(value), 1))
}

/// v2.21.0 — parse `now` `(` `)` `[ (+|-) interval '<n> <unit>' ]`.
/// `tokens[0]` is the `now` word (verified by [`parse_rhs`]).
fn parse_time_value(tokens: &[Token], op: Operator) -> Result<(TimeValue, usize), FilterError> {
    // `LIKE` against a time value is nonsensical (it is a timestamp, not text).
    if op == Operator::Like {
        return Err(FilterError::LikeWithTime);
    }
    let is_sym = |idx: usize, want: &str| {
        matches!(tokens.get(idx), Some(Token::Symbol(s)) if s == want)
    };
    // `now` `(` `)`
    if !is_sym(1, "(") || !is_sym(2, ")") {
        return Err(FilterError::BadTimeValue {
            detail: "expected `now()`".to_string(),
        });
    }
    // optional `± interval '<n> <unit>'`
    let sign = match tokens.get(3) {
        Some(Token::Symbol(s)) if s == "+" => TimeSign::Plus,
        Some(Token::Symbol(s)) if s == "-" => TimeSign::Minus,
        // bare `now()` — no offset.
        _ => return Ok((TimeValue { offset: None }, 3)),
    };
    match tokens.get(4) {
        Some(Token::Word(w)) if w.eq_ignore_ascii_case("interval") => {}
        _ => {
            return Err(FilterError::BadTimeValue {
                detail: "expected `interval` after the `now()` sign".to_string(),
            })
        }
    }
    let raw = match tokens.get(5) {
        Some(Token::Str(s)) => s,
        _ => {
            return Err(FilterError::BadTimeValue {
                detail: "expected a quoted interval like '30 minutes'".to_string(),
            })
        }
    };
    let (amount, unit) = parse_interval(raw)?;
    Ok((TimeValue { offset: Some((sign, amount, unit)) }, 6))
}

/// v2.21.0 — parse + validate the `'<n> <unit>'` interval body into a typed
/// `(u32, TimeUnit)`. Both parts are validated (a `u32` amount, a closed-catalog
/// unit) so the renderer re-emits them structurally — no adopter string reaches
/// SQL text (D4).
fn parse_interval(raw: &str) -> Result<(u32, TimeUnit), FilterError> {
    let mut parts = raw.split_whitespace();
    let (num, unit, extra) = (parts.next(), parts.next(), parts.next());
    let (num, unit) = match (num, unit, extra) {
        (Some(num), Some(unit), None) => (num, unit),
        _ => {
            return Err(FilterError::BadTimeValue {
                detail: format!("interval '{raw}' must be '<number> <unit>'"),
            })
        }
    };
    let amount = num.parse::<u32>().map_err(|_| FilterError::BadTimeValue {
        detail: format!("interval amount '{num}' is not a non-negative integer"),
    })?;
    let unit = TimeUnit::from_word(unit).ok_or_else(|| FilterError::BadTimeValue {
        detail: format!("unknown interval unit '{unit}'"),
    })?;
    Ok((amount, unit))
}

// ════════════════════════════════════════════════════════════════════
//  Parser
// ════════════════════════════════════════════════════════════════════

/// Parse a `where` expression into a [`Filter`] AST. An empty (or
/// whitespace-only) expression yields an empty filter. Total: every
/// input yields a `Filter` or a [`FilterError`].
///
/// v1.32.0 (D3) — `bindings` resolves the Request Binding Contract.
/// The `where` expression is tokenized FIRST (raw), so the boundaries
/// of every string literal are fixed before any value is substituted;
/// THEN each `Token::Str`'s content is interpolated (`${name}` /
/// `$name`) against `bindings`. A request-bound value therefore lives
/// only inside an already-delimited string token — it is rendered as a
/// `$N` bind placeholder by [`build_pg_where`], never spliced into the
/// `where` source. A value carrying a `'`, `;`, `--`, or `OR 1=1`
/// cannot move a literal boundary or inject filter syntax: injection
/// is closed by construction. An empty `bindings` map leaves every
/// `${name}` literal (the pre-37.d behaviour — backwards-compatible).
pub fn parse_filter(
    expr: &str,
    bindings: &std::collections::HashMap<String, String>,
) -> Result<Filter, FilterError> {
    let raw_tokens = tokenize(expr)?;
    // v1.32.0 (D3) — resolve `${name}` ONLY inside already-tokenized
    // string literals; the value can never escape the `Token::Str` it
    // sits in.
    let tokens: Vec<Token> = raw_tokens
        .into_iter()
        .map(|t| match t {
            Token::Str(s) => Token::Str(
                crate::exec_context::interpolate_vars(&s, bindings),
            ),
            other => other,
        })
        .collect();
    let mut filter = Filter::default();
    let mut i = 0;
    let n = tokens.len();

    while i < n {
        // — column —
        let column = match &tokens[i] {
            Token::Word(w) => w.clone(),
            other => {
                return Err(FilterError::ExpectedColumn {
                    found: describe(other),
                })
            }
        };
        if column.len() > MAX_COLUMN_LEN {
            return Err(FilterError::ColumnTooLong {
                len: column.len(),
                column,
            });
        }
        i += 1;

        // — operator —
        if i >= n {
            return Err(FilterError::MissingOperator { column });
        }
        let op = match &tokens[i] {
            Token::Symbol(sym) => Operator::from_symbol(sym).ok_or_else(|| {
                FilterError::ExpectedOperator {
                    column: column.clone(),
                    found: sym.clone(),
                }
            })?,
            Token::Word(w) if w.eq_ignore_ascii_case("like") => Operator::Like,
            other => {
                return Err(FilterError::ExpectedOperator {
                    column,
                    found: describe(other),
                })
            }
        };
        i += 1;

        // value (a bound literal OR a v2.21.0 `now() ± interval` time form,
        //   which spans multiple tokens) —
        if i >= n {
            return Err(FilterError::MissingValue { column });
        }
        let (value, consumed) = parse_rhs(&tokens[i..], &column, op)?;
        i += consumed;

        filter.conditions.push(FilterCondition { column, op, value });
        if filter.conditions.len() > MAX_CONDITIONS {
            return Err(FilterError::TooManyConditions {
                limit: MAX_CONDITIONS,
            });
        }

        // — connector (only between conditions) —
        if i < n {
            let connector = match &tokens[i] {
                Token::Word(w) if w.eq_ignore_ascii_case("and") => Connector::And,
                Token::Word(w) if w.eq_ignore_ascii_case("or") => Connector::Or,
                other => {
                    return Err(FilterError::ExpectedConnector {
                        found: describe(other),
                    })
                }
            };
            i += 1;
            filter.connectors.push(connector);
            // A connector MUST be followed by another condition.
            if i >= n {
                return Err(FilterError::DanglingConnector { connector });
            }
        }
    }

    Ok(filter)
}

// ════════════════════════════════════════════════════════════════════
//  Renderer — parameterized Postgres WHERE
// ════════════════════════════════════════════════════════════════════

/// Compile a `where` expression into a parameterized Postgres `WHERE`
/// clause body and its ordered bind parameters.
///
/// - An empty / whitespace-only expression yields `("TRUE", [])`.
/// - `param_offset` shifts the `$N` numbering: pass the count of bind
///   parameters already consumed by an enclosing statement (e.g. an
///   `UPDATE … SET` list) so the `WHERE` placeholders continue the
///   sequence. `param_offset == 0` yields `$1, $2, …`.
/// - `NULL` values fold into `IS NULL` / `IS NOT NULL` and consume no
///   placeholder; the returned parameter vector therefore never
///   contains [`SqlValue::Null`].
///
/// D4: the column identifier is double-quoted (it is validated against
/// `[A-Za-z_]\w*`, so it cannot carry a quote) and every value is a
/// `$N` placeholder — no user value is ever interpolated into the SQL.
///
/// v1.36.4 — when the column's Postgres type is KNOWN (`column_types`,
/// the `column → udt_name` map), every comparison renders
/// `"col" {op} $N::<type>`, casting the **value** to that type: a
/// `text`-bound value (`uuid`/`int`/`timestamptz` …) compares against
/// its typed column with the native operator — `int = int`,
/// `uuid = uuid` — so equality is exact AND ordering is
/// numeric/temporal (not lexicographic).
///
/// v1.32.0 (D4) — when the column's type is UNKNOWN (introspection
/// found nothing), the rendering depends on the operator. An EQUALITY
/// comparison (`=` / `!=`) renders `"col"::text {op} $N` — casting the
/// COLUMN to `text` so a `text`-bound value compares `text = text`
/// against ANY column type. An ORDERING comparison (`< > <= >=`) and
/// `LIKE` keep the bare `"col" {op} $N`: they need the real type, and a
/// lexicographic ordering miscast is worse than an honest `operator
/// does not exist` failure. The equality cast is a DEGRADED best-effort
/// backstop (exact for canonical-form inputs) — the load-bearing path
/// is the v1.32.0 `pg_catalog` introspection.
pub fn build_pg_where(
    expr: &str,
    param_offset: usize,
    bindings: &std::collections::HashMap<String, String>,
    column_types: &std::collections::HashMap<String, String>,
) -> Result<(String, Vec<SqlValue>), FilterError> {
    if expr.trim().is_empty() {
        return Ok(("TRUE".to_string(), Vec::new()));
    }

    let filter = parse_filter(expr, bindings)?;
    if filter.is_empty() {
        return Ok(("TRUE".to_string(), Vec::new()));
    }

    let mut clause = String::new();
    let mut params: Vec<SqlValue> = Vec::new();
    let mut idx = param_offset + 1;

    for (i, cond) in filter.conditions.iter().enumerate() {
        if i > 0 {
            // The parser guarantees `connectors.len() + 1 ==
            // conditions.len()`, so this index is always in bounds.
            clause.push(' ');
            clause.push_str(filter.connectors[i - 1].as_sql());
            clause.push(' ');
        }
        match &cond.value {
            Rhs::Value(SqlValue::Null) => {
                // The parser guarantees `op ∈ {Eq, Ne}` for a `NULL`
                // value; the `_` arm is defensive totality only.
                let tail = match cond.op {
                    Operator::Ne => "IS NOT NULL",
                    _ => "IS NULL",
                };
                clause.push_str(&format!("\"{}\" {tail}", cond.column));
            }
            // v2.21.0 — a `now()` / `now() ± interval '…'` time value renders
            // INLINE (now()/interval are keywords) with NO `$N` bind and NO
            // `::text` cast: the column compares against the timestamp
            // expression with its native operator (temporal ordering, not
            // lexicographic). D4 holds — `TimeValue::to_sql` emits only a
            // validated `u32` + a whitelisted unit, never an adopter string.
            Rhs::Time(tv) => {
                clause.push_str(&format!(
                    "\"{}\" {} {}",
                    cond.column,
                    cond.op.as_sql(),
                    tv.to_sql()
                ));
            }
            Rhs::Value(bound) => {
                // The column's introspected Postgres type, if known
                // and a safe identifier — drives the cast.
                let known_udt: Option<&str> =
                    match column_types.get(&cond.column) {
                        Some(udt) if is_safe_identifier(udt) => {
                            Some(udt.as_str())
                        }
                        _ => None,
                    };
                // v1.36.4 — KNOWN type → cast the VALUE to it
                // (`"tid" = $1::uuid`): a `text`-bound value compares
                // against its typed column with the native operator —
                // equality exact, ordering numeric/temporal.
                //
                // v1.32.0 (D4) — UNKNOWN type → the rendering
                // depends on the operator:
                //  - EQUALITY (`=` / `!=`) — cast the COLUMN to `text`
                //    (`"col"::text = $N`): `text = text` compares
                //    against ANY column type. Equality has no
                //    lexicographic-vs-native distinction, so the cast
                //    is exact for canonical-form inputs — a DEGRADED
                //    best-effort backstop; the load-bearing path is the
                // v1.32.0 `pg_catalog` introspection.
                //  - ORDERING (`< > <= >=`) and `LIKE` — keep the bare
                //    `"col" {op} $N`: they need the real type, and a
                //    lexicographic ordering miscast is worse than an
                //    honest `operator does not exist` failure.
                let (column_sql, value_cast) = match (known_udt, cond.op) {
                    (Some(udt), _) => {
                        (format!("\"{}\"", cond.column), format!("::{udt}"))
                    }
                    (None, Operator::Eq | Operator::Ne) => {
                        (format!("\"{}\"::text", cond.column), String::new())
                    }
                    (None, _) => {
                        (format!("\"{}\"", cond.column), String::new())
                    }
                };
                clause.push_str(&format!(
                    "{column_sql} {} ${idx}{value_cast}",
                    cond.op.as_sql()
                ));
                params.push(bound.clone());
                idx += 1;
            }
        }
    }

    Ok((clause, params))
}

// ════════════════════════════════════════════════════════════════════
// v2.21.0 — bounded + ordered retrieve (`ORDER BY … LIMIT …`)
// ════════════════════════════════════════════════════════════════════

/// A sort direction. The whitelist IS the catalog — an un-listed
/// direction cannot reach the rendered SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDir {
    Asc,
    Desc,
}

impl OrderDir {
    fn as_sql(self) -> &'static str {
        match self {
            OrderDir::Asc => "ASC",
            OrderDir::Desc => "DESC",
        }
    }
}

/// One `column [asc|desc]` sort key. The column is validated against the
/// v1.30.0 identifier discipline ([`is_safe_identifier`]) and rendered
/// double-quoted — so, exactly as on the `where:` column surface, no
/// untrusted identifier ever reaches SQL text (D4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderKey {
    pub column: String,
    pub dir: OrderDir,
}

/// Parse a closed `order_by:` expression — a comma-separated list of
/// `column [asc|desc]` — into typed [`OrderKey`]s. Total: every input
/// yields the key list or a typed [`FilterError::BadOrderBy`]. An empty
/// (or whitespace-only) expression yields an empty list.
///
/// D4 — the only adopter-varying parts are a validated identifier (the
/// column) + a closed direction keyword; both are re-emitted
/// structurally by [`OrderKey`]'s renderer. No adopter string reaches
/// SQL text.
pub fn parse_order_by(expr: &str) -> Result<Vec<OrderKey>, FilterError> {
    let mut keys = Vec::new();
    if expr.trim().is_empty() {
        return Ok(keys);
    }
    for term in expr.split(',') {
        let mut parts = term.split_whitespace();
        let column = match parts.next() {
            Some(c) => c.to_string(),
            None => {
                return Err(FilterError::BadOrderBy {
                    detail: "an empty sort term (a stray comma?)".to_string(),
                })
            }
        };
        if !is_safe_identifier(&column) {
            return Err(FilterError::BadOrderBy {
                detail: format!(
                    "`{column}` is not a valid column identifier \
                     ([A-Za-z_][A-Za-z0-9_]*, ≤ {MAX_COLUMN_LEN} bytes)"
                ),
            });
        }
        let dir = match parts.next() {
            None => OrderDir::Asc,
            Some(d) if d.eq_ignore_ascii_case("asc") => OrderDir::Asc,
            Some(d) if d.eq_ignore_ascii_case("desc") => OrderDir::Desc,
            Some(other) => {
                return Err(FilterError::BadOrderBy {
                    detail: format!(
                        "direction `{other}` on column `{column}` is not \
                         `asc` or `desc`"
                    ),
                })
            }
        };
        if let Some(extra) = parts.next() {
            return Err(FilterError::BadOrderBy {
                detail: format!(
                    "unexpected `{extra}` after `{column} {}` — a sort term \
                     is `column [asc|desc]`",
                    dir.as_sql().to_ascii_lowercase()
                ),
            });
        }
        keys.push(OrderKey { column, dir });
    }
    Ok(keys)
}

/// Resolve a `limit:` expression to an optional `u32`. An empty (or
/// whitespace-only) expression is `None` (no limit). A `${binding}` /
/// `$binding` is interpolated first; the resolved text must then parse
/// as a `u32` (a non-negative integer in range) — otherwise a typed
/// [`FilterError::BadLimit`].
pub fn parse_limit(
    expr: &str,
    bindings: &std::collections::HashMap<String, String>,
) -> Result<Option<u32>, FilterError> {
    let raw = crate::exec_context::interpolate_vars(expr, bindings);
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    match raw.parse::<u32>() {
        Ok(n) => Ok(Some(n)),
        Err(_) => Err(FilterError::BadLimit {
            detail: format!("`{raw}` is not a non-negative integer in `u32` range"),
        }),
    }
}

/// v2.21.0 — render the structural `ORDER BY … LIMIT …` suffix that a
/// bounded/ordered `retrieve` appends after its `WHERE` clause. Returns
/// the suffix WITH a single leading space (e.g.
/// `" ORDER BY \"t\" ASC LIMIT 100"`), or the empty string when neither
/// clause is present. Injection-free by construction: order columns are
/// validated identifiers + a closed direction keyword; the limit is a
/// re-rendered `u32`. No adopter string is interpolated into SQL text.
pub fn render_bounds(
    order_by: &str,
    limit_expr: &str,
    bindings: &std::collections::HashMap<String, String>,
) -> Result<String, FilterError> {
    let mut suffix = String::new();
    let keys = parse_order_by(order_by)?;
    if !keys.is_empty() {
        suffix.push_str(" ORDER BY ");
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                suffix.push_str(", ");
            }
            suffix.push_str(&format!("\"{}\" {}", k.column, k.dir.as_sql()));
        }
    }
    if let Some(n) = parse_limit(limit_expr, bindings)? {
        suffix.push_str(&format!(" LIMIT {n}"));
    }
    Ok(suffix)
}

// ════════════════════════════════════════════════════════════════════
// v2.33.0 — the closed aggregate catalog (`aggregate:` + `group_by:`)
// ════════════════════════════════════════════════════════════════════

/// v2.33.0 — the CLOSED aggregate-function catalog. The enum IS the
/// whitelist: no path exists by which an un-listed function name reaches
/// rendered SQL (the same D4 discipline as [`Operator`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateFn {
    /// `count` — row count. The only member that takes NO column.
    Count,
    /// `sum(<col>)` — rendered with a `::float8` cast so the wire value
    /// is always a JSON number (Postgres `sum(int)` yields `numeric`,
    /// which the row decoder does not carry).
    Sum,
    /// `avg(<col>)` — `::float8` cast, same rationale as `Sum`.
    Avg,
    /// `min(<col>)` — keeps the column's native type.
    Min,
    /// `max(<col>)` — keeps the column's native type.
    Max,
}

impl AggregateFn {
    /// The surface spelling — doubles as the result-column label
    /// (`count(*) AS "count"`), so the aggregate value's JSON key is
    /// deterministic and closed.
    pub fn label(self) -> &'static str {
        match self {
            AggregateFn::Count => "count",
            AggregateFn::Sum => "sum",
            AggregateFn::Avg => "avg",
            AggregateFn::Min => "min",
            AggregateFn::Max => "max",
        }
    }

    fn from_name(name: &str) -> Option<AggregateFn> {
        Some(match name.to_ascii_lowercase().as_str() {
            "count" => AggregateFn::Count,
            "sum" => AggregateFn::Sum,
            "avg" => AggregateFn::Avg,
            "min" => AggregateFn::Min,
            "max" => AggregateFn::Max,
            _ => return None,
        })
    }

    /// `true` for the aggregates that only make sense over a numeric
    /// column (`sum`/`avg`). Consumed by the v1.31.0 compile-time proof
    /// (axon-T844) when a typed store schema is declared.
    pub fn requires_numeric(self) -> bool {
        matches!(self, AggregateFn::Sum | AggregateFn::Avg)
    }
}

/// v2.33.0 — one parsed `aggregate:` clause: the closed function +
/// its column (`None` only for `count`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateSpec {
    pub func: AggregateFn,
    pub column: Option<String>,
}

/// Parse a closed `aggregate:` expression — `count` (bare) or
/// `sum(<col>)`/`avg(<col>)`/`min(<col>)`/`max(<col>)` — into a typed
/// [`AggregateSpec`]. Total: every input yields `Ok(Some(spec))`,
/// `Ok(None)` (empty/whitespace = no aggregation, the pre-76.d form) or
/// a typed [`FilterError::BadAggregate`].
///
/// D4 — the function name resolves against the CLOSED [`AggregateFn`]
/// catalog and the column must satisfy [`is_safe_identifier`]; both are
/// re-emitted structurally by [`render_aggregate_select`]. No adopter
/// string reaches SQL text.
pub fn parse_aggregate(expr: &str) -> Result<Option<AggregateSpec>, FilterError> {
    let raw = expr.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let (name, column) = match raw.find('(') {
        None => (raw, None),
        Some(open) => {
            if !raw.ends_with(')') {
                return Err(FilterError::BadAggregate {
                    detail: format!("`{raw}` is missing the closing `)`"),
                });
            }
            let name = raw[..open].trim();
            let col = raw[open + 1..raw.len() - 1].trim();
            if col.is_empty() {
                return Err(FilterError::BadAggregate {
                    detail: format!("`{name}(…)` needs a column argument"),
                });
            }
            (name, Some(col.to_string()))
        }
    };
    let func = AggregateFn::from_name(name).ok_or_else(|| FilterError::BadAggregate {
        detail: format!("`{name}` is not an aggregate function"),
    })?;
    match (func, &column) {
        (AggregateFn::Count, Some(col)) => {
            return Err(FilterError::BadAggregate {
                detail: format!(
                    "`count` takes no column (found `{col}`) — it counts rows; \
                     to bound WHICH rows, use `where:`"
                ),
            });
        }
        (AggregateFn::Count, None) => {}
        (_, None) => {
            return Err(FilterError::BadAggregate {
                detail: format!("`{}` needs a column argument", func.label()),
            });
        }
        (_, Some(col)) => {
            if !is_safe_identifier(col) {
                return Err(FilterError::BadAggregate {
                    detail: format!(
                        "`{col}` is not a valid column identifier \
                         ([A-Za-z_][A-Za-z0-9_]*, ≤ {MAX_COLUMN_LEN} bytes)"
                    ),
                });
            }
        }
    }
    Ok(Some(AggregateSpec { func, column }))
}

/// Parse a closed `group_by:` expression — a comma-separated list of
/// column identifiers — into validated column names. Total: every input
/// yields the list (empty input = empty list) or a typed
/// [`FilterError::BadGroupBy`]. The identifier discipline is exactly
/// [`parse_order_by`]'s.
pub fn parse_group_by(expr: &str) -> Result<Vec<String>, FilterError> {
    let mut cols = Vec::new();
    if expr.trim().is_empty() {
        return Ok(cols);
    }
    for term in expr.split(',') {
        let column = term.trim();
        if column.is_empty() {
            return Err(FilterError::BadGroupBy {
                detail: "an empty group term (a stray comma?)".to_string(),
            });
        }
        if !is_safe_identifier(column) {
            return Err(FilterError::BadGroupBy {
                detail: format!(
                    "`{column}` is not a valid column identifier \
                     ([A-Za-z_][A-Za-z0-9_]*, ≤ {MAX_COLUMN_LEN} bytes)"
                ),
            });
        }
        if cols.iter().any(|c| c == column) {
            return Err(FilterError::BadGroupBy {
                detail: format!("column `{column}` appears twice"),
            });
        }
        cols.push(column.to_string());
    }
    Ok(cols)
}

/// v2.33.0 — the single validation entry the engines call: parse +
/// cross-validate the whole aggregate surface of one `retrieve`.
///
/// Rules (all total, all typed errors):
/// - empty `aggregate:` + empty `group_by:` → `Ok(None)` (a plain
///   `SELECT *` retrieve — the pre-76.d form, byte-identical behavior).
/// - `group_by:` without `aggregate:` → [`FilterError::BadGroupBy`]
///   (grouping without an aggregate has no result shape).
/// - `aggregate:` combined with `order_by:`/`limit:` →
///   [`FilterError::BadAggregate`] (v1 closed scope: ordering grouped
///   rows / top-N groups is named deferred scope, rejected loudly
///   rather than silently ignored).
/// - a `group_by:` column equal to the aggregate's column →
///   [`FilterError::BadGroupBy`] (aggregating a grouping key is a
///   tautology; naming it loudly beats a confusing result).
pub fn parse_aggregate_clause(
    aggregate: &str,
    group_by: &str,
    order_by: &str,
    limit_expr: &str,
) -> Result<Option<(AggregateSpec, Vec<String>)>, FilterError> {
    let spec = parse_aggregate(aggregate)?;
    let groups = parse_group_by(group_by)?;
    match spec {
        None => {
            if !groups.is_empty() {
                return Err(FilterError::BadGroupBy {
                    detail: "`group_by:` requires an `aggregate:` \
                             (grouping without an aggregate has no result \
                             shape)"
                        .to_string(),
                });
            }
            Ok(None)
        }
        Some(spec) => {
            if !order_by.trim().is_empty() || !limit_expr.trim().is_empty() {
                return Err(FilterError::BadAggregate {
                    detail: "`aggregate:` cannot combine with `order_by:` / \
                             `limit:` yet (ordering grouped results is \
                             deferred scope) — drop the bounds or the \
                             aggregate"
                        .to_string(),
                });
            }
            if let Some(col) = &spec.column {
                if groups.iter().any(|g| g == col) {
                    return Err(FilterError::BadGroupBy {
                        detail: format!(
                            "column `{col}` is both the aggregate argument \
                             and a group key — aggregate a different column \
                             or drop it from `group_by:`"
                        ),
                    });
                }
            }
            Ok(Some((spec, groups)))
        }
    }
}

/// v2.33.0 — render the structural SELECT list for an aggregate
/// retrieve: the group columns (quoted, declaration order) followed by
/// the aggregate expression labeled with its closed function name.
///
/// ```text
///   count, []                    →  count(*) AS "count"
///   sum(tokens), [industry]      →  "industry", sum("tokens")::float8 AS "sum"
/// ```
///
/// Injection-free by construction: every identifier passed
/// [`is_safe_identifier`] in the parse step; the function spelling and
/// the cast come from the closed enum.
pub fn render_aggregate_select(spec: &AggregateSpec, group_by: &[String]) -> String {
    let mut list = String::new();
    for col in group_by {
        list.push_str(&format!("\"{col}\", "));
    }
    match (spec.func, &spec.column) {
        (AggregateFn::Count, _) => {
            list.push_str("count(*) AS \"count\"");
        }
        (f @ (AggregateFn::Sum | AggregateFn::Avg), Some(col)) => {
            list.push_str(&format!("{0}(\"{col}\")::float8 AS \"{0}\"", f.label()));
        }
        (f, Some(col)) => {
            list.push_str(&format!("{0}(\"{col}\") AS \"{0}\"", f.label()));
        }
        // Unreachable by construction (parse_aggregate rejects a
        // column-less sum/avg/min/max) — kept total anyway.
        (f, None) => {
            list.push_str(&format!("{0}(*) AS \"{0}\"", f.label()));
        }
    }
    list
}

/// v2.33.0 — render the structural ` GROUP BY "a", "b"` suffix (with
/// a single leading space), or the empty string when there are no group
/// columns. Identifiers were validated at parse; re-emitted structurally.
pub fn render_group_by_suffix(group_by: &[String]) -> String {
    if group_by.is_empty() {
        return String::new();
    }
    let mut suffix = String::from(" GROUP BY ");
    for (i, col) in group_by.iter().enumerate() {
        if i > 0 {
            suffix.push_str(", ");
        }
        suffix.push_str(&format!("\"{col}\""));
    }
    suffix
}

// ════════════════════════════════════════════════════════════════════
//  Unit tests
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// Empty bindings — the pre-37.d filter behaviour (no `${name}`
    /// resolution). The v1.32.0 resolution is exercised by the
    /// dedicated `bound` helpers below + `tests/filter_injection.rs`.
    fn nb() -> std::collections::HashMap<String, String> {
        std::collections::HashMap::new()
    }

    /// Empty `column_types` — the unknown-schema fallback (a bare
    /// `"col" {op} $N`, no `::<type>` cast). The v1.36.4 typed cast is
    /// exercised by the dedicated `typed_*` tests below.
    fn nt() -> std::collections::HashMap<String, String> {
        std::collections::HashMap::new()
    }

    fn ok(expr: &str) -> (String, Vec<SqlValue>) {
        build_pg_where(expr, 0, &nb(), &nt())
            .expect("expected the filter to compile")
    }

    fn err(expr: &str) -> FilterError {
        build_pg_where(expr, 0, &nb(), &nt())
            .expect_err("expected a compile error")
    }

    // ── Empty ────────────────────────────────────────────────────────

    #[test]
    fn empty_expression_renders_true() {
        assert_eq!(ok(""), ("TRUE".to_string(), vec![]));
    }

    #[test]
    fn whitespace_only_renders_true() {
        assert_eq!(ok("   \t \n "), ("TRUE".to_string(), vec![]));
    }

    // ── Single conditions, each value type ───────────────────────────

    #[test]
    fn single_integer_condition() {
        // v1.32.0 (D4) — an unknown-type equality casts the column to
        // `text`; the value still rides out as a `$N` bind parameter.
        let (clause, params) = ok("id = 1");
        assert_eq!(clause, "\"id\"::text = $1");
        assert_eq!(params, vec![SqlValue::Integer(1)]);
    }

    #[test]
    fn single_string_condition_single_quoted() {
        let (clause, params) = ok("name = 'Alice'");
        assert_eq!(clause, "\"name\"::text = $1");
        assert_eq!(params, vec![SqlValue::Text("Alice".to_string())]);
    }

    #[test]
    fn single_string_condition_double_quoted() {
        let (_, params) = ok("name = \"Bob\"");
        assert_eq!(params, vec![SqlValue::Text("Bob".to_string())]);
    }

    #[test]
    fn negative_integer_value() {
        let (clause, params) = ok("balance >= -100");
        assert_eq!(clause, "\"balance\" >= $1");
        assert_eq!(params, vec![SqlValue::Integer(-100)]);
    }

    #[test]
    fn float_value() {
        let (_, params) = ok("score > 3.14");
        assert_eq!(params, vec![SqlValue::Float(3.14)]);
    }

    #[test]
    fn boolean_values() {
        assert_eq!(ok("active = true").1, vec![SqlValue::Boolean(true)]);
        assert_eq!(ok("active = false").1, vec![SqlValue::Boolean(false)]);
    }

    #[test]
    fn integer_overflowing_i64_falls_back_to_float() {
        // 10^25 does not fit i64 — must compile as a finite float.
        let (_, params) = ok("n = 10000000000000000000000000");
        assert!(matches!(params[0], SqlValue::Float(_)));
    }

    // ── Operators ────────────────────────────────────────────────────

    #[test]
    fn every_operator_renders_canonically() {
        // v1.32.0 (D4) — equality on an unknown-type column casts the
        // column to `text`; ordering + LIKE keep the bare placeholder.
        assert_eq!(ok("a = 1").0, "\"a\"::text = $1");
        assert_eq!(ok("a != 1").0, "\"a\"::text != $1");
        assert_eq!(ok("a > 1").0, "\"a\" > $1");
        assert_eq!(ok("a >= 1").0, "\"a\" >= $1");
        assert_eq!(ok("a < 1").0, "\"a\" < $1");
        assert_eq!(ok("a <= 1").0, "\"a\" <= $1");
        assert_eq!(ok("a LIKE 'x%'").0, "\"a\" LIKE $1");
    }

    #[test]
    fn operator_aliases_normalize() {
        // `==` → `=`, `<>` → `!=`. Both are equality → D4 `::text`.
        assert_eq!(ok("a == 1").0, "\"a\"::text = $1");
        assert_eq!(ok("a <> 1").0, "\"a\"::text != $1");
    }

    #[test]
    fn like_is_case_insensitive_and_renders_uppercase() {
        assert_eq!(ok("a like 'x%'").0, "\"a\" LIKE $1");
        assert_eq!(ok("a LiKe 'x%'").0, "\"a\" LIKE $1");
    }

    // ── v1.36.4 — typed-column filter (value cast) ──────────────────

    /// A known column type casts the VALUE placeholder to that type —
    /// `"tid" = $1::uuid` — so a `text`-bound value (e.g. a `${param}`
    /// resolved from the v1.32.0 Request Binding Contract) compares
    /// against a `uuid` column with the native `uuid = uuid` operator.
    /// This is the read-side mirror of v1.36.2's write cast.
    #[test]
    fn typed_column_comparison_casts_the_value_to_the_column_type() {
        let b = std::collections::HashMap::from([(
            "tid".to_string(),
            "83d078e1-b372-42ba-9572-ff8dc521386e".to_string(),
        )]);
        let types = std::collections::HashMap::from([
            ("tid".to_string(), "uuid".to_string()),
            ("age".to_string(), "int4".to_string()),
        ]);
        let (clause, params) =
            build_pg_where("tid = '${tid}'", 0, &b, &types).expect("compiles");
        assert_eq!(
            clause, "\"tid\" = $1::uuid",
            "the value is cast to the column's introspected type"
        );
        assert_eq!(
            params,
            vec![SqlValue::Text(
                "83d078e1-b372-42ba-9572-ff8dc521386e".to_string()
            )]
        );
        // An `int4` column — ordering stays numeric, not lexicographic.
        let (clause, _) =
            build_pg_where("age >= 18", 0, &nb(), &types).expect("compiles");
        assert_eq!(clause, "\"age\" >= $1::int4");
    }

    // ── v1.32.0 — D4 equality type-agnostic fallback ────────────

    /// v1.32.0 (D4) — an unknown-type EQUALITY (`=` / `!=`) casts the
    /// COLUMN to `text`: `"col"::text = $N`. A `text`-bound value then
    /// compares `text = text` against ANY column type (`uuid`, `int`,
    /// `timestamptz`, `bool`, `text`) — equality has no
    /// lexicographic-vs-native distinction, so the cast is exact for
    /// canonical-form inputs. The degraded backstop for when
    /// introspection found nothing.
    #[test]
    fn d4_unknown_type_equality_casts_the_column_to_text() {
        assert_eq!(ok("id == 'x'").0, "\"id\"::text = $1");
        assert_eq!(ok("id != 'x'").0, "\"id\"::text != $1");
        // `=` is the same operator as `==`.
        assert_eq!(ok("id = 1").0, "\"id\"::text = $1");
    }

    /// v1.32.0 (D4) — an unknown-type ORDERING comparison keeps the
    /// bare `"col" {op} $N`: ordering genuinely needs the real type (a
    /// lexicographic miscast is worse than an honest failure).
    #[test]
    fn d4_unknown_type_ordering_stays_a_bare_placeholder() {
        assert_eq!(ok("age > 18").0, "\"age\" > $1");
        assert_eq!(ok("age >= 18").0, "\"age\" >= $1");
        assert_eq!(ok("age < 18").0, "\"age\" < $1");
        assert_eq!(ok("age <= 18").0, "\"age\" <= $1");
    }

    /// v1.32.0 (D4) — `LIKE` on an unknown-type column keeps the bare
    /// placeholder (it needs a real text column type).
    #[test]
    fn d4_unknown_type_like_stays_a_bare_placeholder() {
        assert_eq!(ok("name LIKE 'a%'").0, "\"name\" LIKE $1");
    }

    /// v1.32.0 (D4) does NOT touch the known-type path — a known
    /// column type still casts the VALUE (`$N::udt`), every operator.
    #[test]
    fn d4_a_known_type_keeps_the_v1_36_4_value_cast() {
        let types = std::collections::HashMap::from([
            ("id".to_string(), "uuid".to_string()),
            ("n".to_string(), "int4".to_string()),
        ]);
        assert_eq!(
            build_pg_where("id == 'x'", 0, &nb(), &types).unwrap().0,
            "\"id\" = $1::uuid"
        );
        assert_eq!(
            build_pg_where("n > 5", 0, &nb(), &types).unwrap().0,
            "\"n\" > $1::int4"
        );
    }

    /// v1.32.0 (D4) — an unsafe `udt_name` (defensive — `pg_catalog`
    /// never yields one) is treated as UNKNOWN: it is never spliced,
    /// and an equality still works via the `"col"::text` fallback.
    #[test]
    fn d4_an_unsafe_udt_is_not_spliced_and_equality_still_works() {
        let types = std::collections::HashMap::from([(
            "id".to_string(),
            "int4; DROP TABLE x".to_string(),
        )]);
        let (clause, _) =
            build_pg_where("id = 1", 0, &nb(), &types).expect("compiles");
        assert_eq!(
            clause, "\"id\"::text = $1",
            "the unsafe udt is not spliced; equality falls back to ::text"
        );
        // An ordering comparison with the unsafe udt stays bare.
        let (clause, _) =
            build_pg_where("id > 1", 0, &nb(), &types).expect("compiles");
        assert_eq!(clause, "\"id\" > $1");
    }

    #[test]
    fn typed_column_null_fold_is_not_cast() {
        // The NULL fold is uncast — `IS NULL` is type-agnostic.
        assert_eq!(ok("id = null").0, "\"id\" IS NULL");
    }

    // ── Connectors + multi-condition ─────────────────────────────────

    #[test]
    fn two_conditions_joined_by_and() {
        // v1.32.0 (D4) — each unknown-type equality casts its column to
        // `text`; the connector rendering is unchanged.
        let (clause, params) = ok("id = 1 AND name = 'Alice'");
        assert_eq!(clause, "\"id\"::text = $1 AND \"name\"::text = $2");
        assert_eq!(
            params,
            vec![SqlValue::Integer(1), SqlValue::Text("Alice".to_string())]
        );
    }

    #[test]
    fn two_conditions_joined_by_or() {
        assert_eq!(
            ok("a = 1 OR b = 2").0,
            "\"a\"::text = $1 OR \"b\"::text = $2"
        );
    }

    #[test]
    fn connectors_are_case_insensitive() {
        assert_eq!(
            ok("a = 1 and b = 2").0,
            "\"a\"::text = $1 AND \"b\"::text = $2"
        );
        assert_eq!(
            ok("a = 1 Or b = 2").0,
            "\"a\"::text = $1 OR \"b\"::text = $2"
        );
    }

    #[test]
    fn three_condition_mixed_chain_preserves_order() {
        let (clause, params) = ok("a = 1 AND b = 2 OR c = 3");
        assert_eq!(
            clause,
            "\"a\"::text = $1 AND \"b\"::text = $2 OR \"c\"::text = $3"
        );
        assert_eq!(params.len(), 3);
    }

    // ── NULL folding ─────────────────────────────────────────────────

    #[test]
    fn null_equality_folds_to_is_null() {
        let (clause, params) = ok("deleted_at = null");
        assert_eq!(clause, "\"deleted_at\" IS NULL");
        assert!(params.is_empty(), "IS NULL consumes no bind parameter");
    }

    #[test]
    fn null_inequality_folds_to_is_not_null() {
        let (clause, params) = ok("deleted_at != NULL");
        assert_eq!(clause, "\"deleted_at\" IS NOT NULL");
        assert!(params.is_empty());
    }

    #[test]
    fn null_does_not_occupy_a_parameter_slot() {
        // `a` is NULL (folded, no slot) so `b` takes $1, not $2.
        // v1.32.0 (D4) — `b = 5` casts the column to `text`; the NULL
        // fold is type-agnostic already and is never cast.
        let (clause, params) = ok("a = null AND b = 5");
        assert_eq!(clause, "\"a\" IS NULL AND \"b\"::text = $1");
        assert_eq!(params, vec![SqlValue::Integer(5)]);
    }

    #[test]
    fn rendered_params_never_contain_null() {
        let (_, params) = ok("a = null AND b = 1 OR c != null");
        assert!(params.iter().all(|v| !matches!(v, SqlValue::Null)));
    }

    // ── param_offset ─────────────────────────────────────────────────

    #[test]
    fn param_offset_shifts_placeholder_numbering() {
        let (clause, _) = build_pg_where("id = 1", 2, &nb(), &nt()).unwrap();
        assert_eq!(clause, "\"id\"::text = $3");
    }

    #[test]
    fn param_offset_shifts_every_placeholder() {
        let (clause, _) =
            build_pg_where("a = 1 AND b = 2", 5, &nb(), &nt()).unwrap();
        assert_eq!(clause, "\"a\"::text = $6 AND \"b\"::text = $7");
    }

    // ── D4 — SQL-injection resistance ────────────────────────────────

    #[test]
    fn injection_payload_inside_a_quoted_string_is_an_inert_bind_param() {
        // The classic payload — fully contained in a string literal —
        // compiles to ONE harmless bound parameter. The SQL structure
        // is `"name"::text = $1` (v1.32.0 D4 equality cast); the
        // payload never reaches SQL text.
        let (clause, params) = ok("name = '; DROP TABLE users; --'");
        assert_eq!(clause, "\"name\"::text = $1");
        assert_eq!(
            params,
            vec![SqlValue::Text("; DROP TABLE users; --".to_string())]
        );
    }

    #[test]
    fn injection_via_statement_terminator_is_rejected_at_tokenize() {
        // A bare `;` is outside the grammar alphabet.
        assert!(matches!(
            err("name = 'x'; DROP TABLE users"),
            FilterError::UnexpectedChar { ch: ';', .. }
        ));
    }

    #[test]
    fn injection_via_comment_marker_is_rejected() {
        // v2.21.0 — a standalone `-` now lexes as a Symbol (it is the
        // interval sign in the `now() ± interval` time form). So `a = 1 --
        // comment` no longer fails at tokenize with `UnexpectedChar`; instead
        // the `-` after a COMPLETE condition is an unexpected connector. STILL
        // rejected — the `--comment` never reaches SQL — only the error variant
        // changed. (A `-` is meaningful ONLY inside the time-value form; in any
        // other position it is a typed error, so no injection vector opens.)
        assert!(matches!(
            err("a = 1 -- comment"),
            FilterError::ExpectedConnector { .. }
        ));
    }

    #[test]
    fn injection_via_bare_or_tautology_is_rejected() {
        // `name = 'x' OR 1=1` — after the OR, `1` is in column
        // position and is not an identifier.
        assert!(matches!(
            err("name = 'x' OR 1 = 1"),
            FilterError::ExpectedColumn { .. }
        ));
    }

    // ── String escapes ──────────────────────────────────────────────

    #[test]
    fn escaped_quote_inside_string_is_resolved() {
        let (_, params) = ok("name = 'O\\'Brien'");
        assert_eq!(params, vec![SqlValue::Text("O'Brien".to_string())]);
    }

    #[test]
    fn escaped_backslash_is_resolved() {
        let (_, params) = ok("path = 'a\\\\b'");
        assert_eq!(params, vec![SqlValue::Text("a\\b".to_string())]);
    }

    // ── Lexical errors ──────────────────────────────────────────────

    #[test]
    fn unterminated_string_errors() {
        assert!(matches!(
            err("name = 'unclosed"),
            FilterError::UnterminatedString { .. }
        ));
    }

    #[test]
    fn unexpected_character_errors() {
        assert!(matches!(
            err("a = 1 & b = 2"),
            FilterError::UnexpectedChar { ch: '&', .. }
        ));
    }

    #[test]
    fn bare_bang_is_rejected() {
        assert!(matches!(
            err("a ! 1"),
            FilterError::UnexpectedChar { ch: '!', .. }
        ));
    }

    #[test]
    fn invalid_number_errors() {
        assert!(matches!(
            err("a = 1.2.3"),
            FilterError::InvalidNumber { .. }
        ));
    }

    // ── Structural / parse errors ────────────────────────────────────

    #[test]
    fn missing_operator_errors() {
        assert!(matches!(err("id"), FilterError::MissingOperator { .. }));
    }

    #[test]
    fn missing_value_errors() {
        assert!(matches!(err("id ="), FilterError::MissingValue { .. }));
    }

    #[test]
    fn unquoted_string_value_errors() {
        assert!(matches!(
            err("status = active"),
            FilterError::UnquotedValue { .. }
        ));
    }

    #[test]
    fn dangling_connector_errors() {
        // The exact defect the frozen Python reference mis-renders.
        assert!(matches!(
            err("id = 1 AND"),
            FilterError::DanglingConnector {
                connector: Connector::And
            }
        ));
    }

    #[test]
    fn two_conditions_without_connector_errors() {
        assert!(matches!(
            err("a = 1 b = 2"),
            FilterError::ExpectedConnector { .. }
        ));
    }

    #[test]
    fn column_position_non_identifier_errors() {
        assert!(matches!(err("1 = 1"), FilterError::ExpectedColumn { .. }));
    }

    #[test]
    fn operator_position_non_operator_errors() {
        assert!(matches!(
            err("a b c"),
            FilterError::ExpectedOperator { .. }
        ));
    }

    // ── Semantic errors ─────────────────────────────────────────────

    #[test]
    fn null_with_ordering_operator_errors() {
        assert!(matches!(
            err("score > null"),
            FilterError::NullWithNonEqualityOp { op: Operator::Gt, .. }
        ));
    }

    #[test]
    fn null_with_like_errors() {
        assert!(matches!(
            err("name LIKE null"),
            FilterError::NullWithNonEqualityOp { op: Operator::Like, .. }
        ));
    }

    #[test]
    fn like_with_non_text_value_errors() {
        assert!(matches!(
            err("age LIKE 5"),
            FilterError::LikeRequiresText { found: "integer", .. }
        ));
    }

    // ── Bounds ───────────────────────────────────────────────────────

    #[test]
    fn column_at_the_length_limit_compiles() {
        let col = "c".repeat(MAX_COLUMN_LEN);
        assert!(build_pg_where(&format!("{col} = 1"), 0, &nb(), &nt()).is_ok());
    }

    #[test]
    fn column_over_the_length_limit_errors() {
        let col = "c".repeat(MAX_COLUMN_LEN + 1);
        assert!(matches!(
            build_pg_where(&format!("{col} = 1"), 0, &nb(), &nt()),
            Err(FilterError::ColumnTooLong { .. })
        ));
    }

    #[test]
    fn condition_count_at_the_limit_compiles() {
        let expr = (0..MAX_CONDITIONS)
            .map(|i| format!("c{i} = {i}"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let (_, params) = build_pg_where(&expr, 0, &nb(), &nt()).unwrap();
        assert_eq!(params.len(), MAX_CONDITIONS);
    }

    #[test]
    fn condition_count_over_the_limit_errors() {
        let expr = (0..=MAX_CONDITIONS)
            .map(|i| format!("c{i} = {i}"))
            .collect::<Vec<_>>()
            .join(" AND ");
        assert!(matches!(
            build_pg_where(&expr, 0, &nb(), &nt()),
            Err(FilterError::TooManyConditions { .. })
        ));
    }

    // ── AST shape via parse_filter ───────────────────────────────────

    #[test]
    fn parse_filter_exposes_the_typed_ast() {
        let filter = parse_filter("id = 1 AND name LIKE 'A%'", &nb()).unwrap();
        assert_eq!(filter.conditions.len(), 2);
        assert_eq!(filter.connectors, vec![Connector::And]);
        assert_eq!(
            filter.conditions[0],
            FilterCondition {
                column: "id".to_string(),
                op: Operator::Eq,
                value: Rhs::Value(SqlValue::Integer(1)),
            }
        );
        assert_eq!(filter.conditions[1].op, Operator::Like);
        assert_eq!(
            filter.conditions[1].value,
            Rhs::Value(SqlValue::Text("A%".to_string()))
        );
    }

    // ─── v2.21.0 — time-relative `where:` values ──────────────────────

    #[test]
    fn time_bare_now_renders_inline_with_no_bind() {
        let (clause, params) = ok("last_activity_at < now()");
        assert_eq!(clause, "\"last_activity_at\" < now()");
        assert!(params.is_empty(), "now() is structural — no $N bind");
    }

    #[test]
    fn time_now_minus_interval_renders_structurally() {
        let (clause, params) = ok("last_activity_at < now() - interval '30 minutes'");
        assert_eq!(clause, "\"last_activity_at\" < now() - interval '30 minute'");
        assert!(params.is_empty(), "the time value adds no bind param");
    }

    #[test]
    fn time_value_covers_signs_and_units() {
        assert_eq!(ok("t > now() + interval '7 days'").0, "\"t\" > now() + interval '7 day'");
        assert_eq!(ok("t <= now() - interval '1 hour'").0, "\"t\" <= now() - interval '1 hour'");
        assert_eq!(ok("t >= now() - interval '2 weeks'").0, "\"t\" >= now() - interval '2 week'");
        assert_eq!(ok("t != now()").0, "\"t\" != now()");
        // singular + plural both accepted; rendered to the canonical singular.
        assert_eq!(ok("t < now() - interval '1 minute'").0, "\"t\" < now() - interval '1 minute'");
        assert_eq!(ok("t < now() - interval '90 seconds'").0, "\"t\" < now() - interval '90 second'");
    }

    #[test]
    fn session_sweep_clause_binds_only_the_status_literal() {
        // The canonical autonomous-sweep predicate ("active rows that went
        // stale"), which used to fail at tokenization (`UnexpectedChar '('`).
        // Now it compiles: only the status literal is a `$N` bind; the time
        // value is structural.
        let (clause, params) =
            ok("status == 'ACTIVE' AND last_activity_at < now() - interval '30 minutes'");
        assert_eq!(
            clause,
            "\"status\"::text = $1 AND \"last_activity_at\" < now() - interval '30 minute'"
        );
        assert_eq!(params, vec![SqlValue::Text("ACTIVE".to_string())]);
    }

    #[test]
    fn time_value_rejects_malformed_forms_loudly() {
        // Every malformed time form is a typed error — never silent 0 rows.
        for bad in [
            "t < now( - interval '5 minutes'",        // missing `)`
            "t < now() - interval '30'",              // no unit
            "t < now() - interval '30 fortnights'",   // unknown unit
            "t < now() - interval 'abc minutes'",     // non-integer amount
            "t < now() - interval '-5 minutes'",      // negative amount
            "t < now() - 'interval string'",          // no `interval` keyword
        ] {
            assert!(
                matches!(err(bad), FilterError::BadTimeValue { .. }),
                "expected BadTimeValue for {bad:?}, got {:?}",
                err(bad)
            );
        }
    }

    #[test]
    fn like_against_a_time_value_is_rejected() {
        assert!(matches!(err("t LIKE now()"), FilterError::LikeWithTime));
    }

    #[test]
    fn time_value_is_injection_free() {
        // The amount is a re-rendered u32 + a whitelisted unit; an attempt to
        // smuggle SQL through the interval body is a typed error, never SQL text.
        assert!(matches!(
            err("t < now() - interval '5 minutes); DROP TABLE x;--'"),
            FilterError::BadTimeValue { .. }
        ));
    }

    #[test]
    fn parse_filter_invariant_connectors_plus_one_equals_conditions() {
        for expr in ["a = 1", "a = 1 AND b = 2", "a = 1 OR b = 2 AND c = 3"] {
            let f = parse_filter(expr, &nb()).unwrap();
            assert_eq!(f.connectors.len() + 1, f.conditions.len());
        }
    }

    #[test]
    fn empty_filter_is_empty() {
        assert!(parse_filter("", &nb()).unwrap().is_empty());
        assert!(parse_filter("  ", &nb()).unwrap().is_empty());
        assert!(!parse_filter("a = 1", &nb()).unwrap().is_empty());
    }

    // ── is_safe_identifier ───────────────────────────────────────────

    #[test]
    fn safe_identifiers_are_accepted() {
        for name in ["users", "user_id", "_private", "Table1", "a", "_"] {
            assert!(is_safe_identifier(name), "`{name}` should be safe");
        }
    }

    #[test]
    fn unsafe_identifiers_are_rejected() {
        for name in [
            "",
            "1abc",            // starts with a digit
            "user-name",       // hyphen
            "a b",             // space
            "drop;table",      // statement terminator
            "col\"injected",   // embedded quote
            "naïve",           // non-ASCII
        ] {
            assert!(!is_safe_identifier(name), "`{name}` should be rejected");
        }
    }

    #[test]
    fn identifier_length_boundary() {
        assert!(is_safe_identifier(&"c".repeat(MAX_COLUMN_LEN)));
        assert!(!is_safe_identifier(&"c".repeat(MAX_COLUMN_LEN + 1)));
    }

    // ── Error Display is non-empty + informative ─────────────────────

    #[test]
    fn every_error_has_a_non_empty_display() {
        let samples = [
            err("a = 1 ;"),
            err("a = 'x"),
            err("a = 1.2.3"),
            err("1 = 1"),
            err("id"),
            err("id ="),
            err("a = b"),
            err("a = 1 b = 2"),
            err("a = 1 AND"),
            err("a > null"),
            err("a LIKE 5"),
        ];
        for e in samples {
            assert!(!e.to_string().is_empty());
        }
    }

    // ─── v2.21.0 — bounded + ordered retrieve (`ORDER BY … LIMIT …`) ──

    #[test]
    fn order_by_single_column_defaults_to_asc() {
        let keys = parse_order_by("last_activity_at").unwrap();
        assert_eq!(
            keys,
            vec![OrderKey {
                column: "last_activity_at".to_string(),
                dir: OrderDir::Asc,
            }]
        );
    }

    #[test]
    fn order_by_multi_column_with_directions() {
        let keys = parse_order_by("last_activity_at desc, id ASC").unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].dir, OrderDir::Desc);
        assert_eq!(keys[1].column, "id");
        assert_eq!(keys[1].dir, OrderDir::Asc);
    }

    #[test]
    fn order_by_empty_is_no_keys() {
        assert!(parse_order_by("").unwrap().is_empty());
        assert!(parse_order_by("   ").unwrap().is_empty());
    }

    #[test]
    fn order_by_rejects_bad_direction_and_unsafe_identifier() {
        assert!(matches!(
            parse_order_by("id sideways"),
            Err(FilterError::BadOrderBy { .. })
        ));
        assert!(matches!(
            parse_order_by("id; DROP TABLE x"),
            Err(FilterError::BadOrderBy { .. })
        ));
        assert!(matches!(
            parse_order_by("a asc b"), // extra token
            Err(FilterError::BadOrderBy { .. })
        ));
        assert!(matches!(
            parse_order_by("a,,b"), // empty term
            Err(FilterError::BadOrderBy { .. })
        ));
    }

    #[test]
    fn limit_literal_and_binding_resolve_to_u32() {
        assert_eq!(parse_limit("100", &nb()).unwrap(), Some(100));
        assert_eq!(parse_limit("", &nb()).unwrap(), None);
        let b = std::collections::HashMap::from([("max".to_string(), "50".to_string())]);
        assert_eq!(parse_limit("${max}", &b).unwrap(), Some(50));
    }

    #[test]
    fn limit_rejects_non_u32() {
        for bad in ["-5", "abc", "3.5", "99999999999999999999"] {
            assert!(
                matches!(parse_limit(bad, &nb()), Err(FilterError::BadLimit { .. })),
                "`{bad}` should be BadLimit"
            );
        }
    }

    #[test]
    fn render_bounds_emits_structural_order_and_limit() {
        let suffix = render_bounds(
            "last_activity_at asc, id desc",
            "100",
            &nb(),
        )
        .unwrap();
        assert_eq!(
            suffix,
            " ORDER BY \"last_activity_at\" ASC, \"id\" DESC LIMIT 100"
        );
        // order_by only.
        assert_eq!(
            render_bounds("id desc", "", &nb()).unwrap(),
            " ORDER BY \"id\" DESC"
        );
        // limit only.
        assert_eq!(render_bounds("", "25", &nb()).unwrap(), " LIMIT 25");
        // neither.
        assert_eq!(render_bounds("", "", &nb()).unwrap(), "");
    }

    #[test]
    fn render_bounds_is_injection_free() {
        // An order column carrying SQL is rejected (not spliced).
        assert!(render_bounds("id; DROP TABLE x", "", &nb()).is_err());
        // A limit carrying SQL is rejected at the u32 parse.
        assert!(render_bounds("", "1; DROP TABLE x", &nb()).is_err());
    }

    // ── v2.33.0 — the closed aggregate catalog ────────────────────

    #[test]
    fn aggregate_count_parses_bare_and_rejects_column() {
        let spec = parse_aggregate("count").unwrap().unwrap();
        assert_eq!(spec.func, AggregateFn::Count);
        assert_eq!(spec.column, None);
        // Case-insensitive + whitespace-tolerant.
        assert!(parse_aggregate("  COUNT  ").unwrap().is_some());
        // count(col) is a typed error, not a silent accept.
        assert!(matches!(
            parse_aggregate("count(id)"),
            Err(FilterError::BadAggregate { .. })
        ));
    }

    #[test]
    fn aggregate_column_functions_parse_and_validate_identifier() {
        for (expr, func) in [
            ("sum(tokens)", AggregateFn::Sum),
            ("avg(latency_ms)", AggregateFn::Avg),
            ("min(created_at)", AggregateFn::Min),
            ("max(created_at)", AggregateFn::Max),
        ] {
            let spec = parse_aggregate(expr).unwrap().unwrap();
            assert_eq!(spec.func, func);
            assert!(spec.column.is_some());
        }
        // A column-less sum, an unknown fn, a bad identifier, a missing
        // paren — all typed errors.
        for bad in ["sum", "sum()", "median(x)", "sum(1id)", "sum(x"] {
            assert!(
                matches!(parse_aggregate(bad), Err(FilterError::BadAggregate { .. })),
                "`{bad}` must be a BadAggregate"
            );
        }
        // Empty = no aggregation (the pre-76.d form).
        assert_eq!(parse_aggregate("").unwrap(), None);
        assert_eq!(parse_aggregate("   ").unwrap(), None);
    }

    #[test]
    fn group_by_parses_and_rejects_bad_terms() {
        assert_eq!(
            parse_group_by("industry, status").unwrap(),
            vec!["industry".to_string(), "status".to_string()]
        );
        assert!(parse_group_by("").unwrap().is_empty());
        for bad in ["a,,b", "1bad", "col; DROP TABLE x", "a, a"] {
            assert!(
                matches!(parse_group_by(bad), Err(FilterError::BadGroupBy { .. })),
                "`{bad}` must be a BadGroupBy"
            );
        }
    }

    #[test]
    fn aggregate_clause_cross_rules() {
        // group_by without aggregate.
        assert!(matches!(
            parse_aggregate_clause("", "industry", "", ""),
            Err(FilterError::BadGroupBy { .. })
        ));
        // aggregate with order_by / limit (v1 closed scope).
        assert!(matches!(
            parse_aggregate_clause("count", "", "id asc", ""),
            Err(FilterError::BadAggregate { .. })
        ));
        assert!(matches!(
            parse_aggregate_clause("count", "", "", "10"),
            Err(FilterError::BadAggregate { .. })
        ));
        // aggregate column as group key.
        assert!(matches!(
            parse_aggregate_clause("sum(tokens)", "tokens", "", ""),
            Err(FilterError::BadGroupBy { .. })
        ));
        // The happy paths.
        assert!(parse_aggregate_clause("", "", "id asc", "10").unwrap().is_none());
        let (spec, groups) = parse_aggregate_clause("count", "industry", "", "")
            .unwrap()
            .unwrap();
        assert_eq!(spec.func, AggregateFn::Count);
        assert_eq!(groups, vec!["industry".to_string()]);
    }

    #[test]
    fn aggregate_select_renders_structurally() {
        let count = AggregateSpec { func: AggregateFn::Count, column: None };
        assert_eq!(render_aggregate_select(&count, &[]), "count(*) AS \"count\"");
        let sum = AggregateSpec {
            func: AggregateFn::Sum,
            column: Some("tokens".to_string()),
        };
        assert_eq!(
            render_aggregate_select(&sum, &["industry".to_string()]),
            "\"industry\", sum(\"tokens\")::float8 AS \"sum\"",
            "sum/avg carry the ::float8 cast (numeric → JSON number)"
        );
        let max = AggregateSpec {
            func: AggregateFn::Max,
            column: Some("created_at".to_string()),
        };
        assert_eq!(
            render_aggregate_select(&max, &[]),
            "max(\"created_at\") AS \"max\"",
            "min/max keep the column's native type (no cast)"
        );
        assert_eq!(render_group_by_suffix(&[]), "");
        assert_eq!(
            render_group_by_suffix(&["a".to_string(), "b".to_string()]),
            " GROUP BY \"a\", \"b\""
        );
    }

    #[test]
    fn aggregate_surface_is_injection_free() {
        // Every adopter-varying part is rejected before rendering: the
        // renderers only ever see enum members + validated identifiers.
        assert!(parse_aggregate("sum(x); DROP TABLE t").is_err());
        assert!(parse_aggregate("sum(\"x\")").is_err()); // quotes are not identifier chars
        assert!(parse_group_by("x\"; DROP TABLE t; --").is_err());
    }
}
