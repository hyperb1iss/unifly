//! The `--filter` expression list commands share.
//!
//! Parsing lives with the arguments so a malformed expression fails argument
//! parsing like any other bad value; matching lives with the list helpers.

/// A `--filter` expression, parsed once when the arguments are.
///
/// The grammar is `field.eq('value')`, `field.contains('value')`, or
/// `field.in('a','b')`. An empty expression matches everything, so
/// `--filter "$EXPR"` with an empty variable lists the whole collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListFilter {
    pub expr: Option<FilterExpr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterExpr {
    pub path: Vec<String>,
    pub op: FilterOp,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterOp {
    Eq,
    Contains,
    In,
}

impl ListFilter {
    /// Clap value parser: a malformed expression fails argument parsing, so
    /// it never reaches a list as a filter that silently matches nothing.
    pub fn parse(expr: &str) -> Result<Self, String> {
        if expr.trim().is_empty() {
            return Ok(Self { expr: None });
        }
        parse_filter_expr(expr)
            .map(|parsed| Self { expr: Some(parsed) })
            .ok_or_else(|| {
                "expected field.eq('value'), field.contains('value'), or field.in('a','b')"
                    .to_owned()
            })
    }
}

fn parse_filter_expr(filter: &str) -> Option<FilterExpr> {
    let filter = filter.trim();
    if filter.is_empty() {
        return None;
    }

    let open_paren = filter.find('(')?;
    let close_paren = filter.rfind(')')?;
    if close_paren != filter.len().saturating_sub(1) || close_paren <= open_paren {
        return None;
    }

    let head = filter[..open_paren].trim();
    let args = filter[open_paren + 1..close_paren].trim();
    let dot = head.rfind('.')?;
    let path = head[..dot]
        .split('.')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if path.is_empty() {
        return None;
    }

    let op = match head[dot + 1..].trim().to_ascii_lowercase().as_str() {
        "eq" => FilterOp::Eq,
        "contains" => FilterOp::Contains,
        "in" => FilterOp::In,
        _ => return None,
    };

    let values = parse_quoted_values(args)?;
    match op {
        FilterOp::Eq | FilterOp::Contains if values.len() != 1 => return None,
        FilterOp::In if values.is_empty() => return None,
        _ => {}
    }

    Some(FilterExpr { path, op, values })
}

fn parse_quoted_values(input: &str) -> Option<Vec<String>> {
    let mut values = Vec::new();
    let mut chars = input.chars().peekable();

    loop {
        while matches!(chars.peek(), Some(ch) if ch.is_whitespace()) {
            chars.next();
        }

        if chars.peek().is_none() {
            break;
        }

        let quote = match chars.next()? {
            '\'' => '\'',
            '"' => '"',
            _ => return None,
        };

        let mut value = String::new();
        let mut escaped = false;
        let mut closed = false;
        for ch in chars.by_ref() {
            if escaped {
                let decoded = match ch {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '\\' => '\\',
                    '\'' => '\'',
                    '"' => '"',
                    other => other,
                };
                value.push(decoded);
                escaped = false;
                continue;
            }

            match ch {
                '\\' => escaped = true,
                c if c == quote => {
                    closed = true;
                    break;
                }
                other => value.push(other),
            }
        }

        if !closed || escaped {
            return None;
        }

        values.push(value);

        while matches!(chars.peek(), Some(ch) if ch.is_whitespace()) {
            chars.next();
        }

        match chars.peek() {
            Some(',') => {
                chars.next();
            }
            None => break,
            _ => return None,
        }
    }

    Some(values)
}
