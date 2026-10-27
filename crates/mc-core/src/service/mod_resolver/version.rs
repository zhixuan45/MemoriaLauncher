//! Conservative Fabric predicates and Maven interval constraints. Unsupported syntax stays unknown.
use super::{Constraint, Dialect};
use semver::Version;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Yes,
    No,
    Unknown,
}
fn all(values: impl Iterator<Item = Match>) -> Match {
    let mut unknown = false;
    for value in values {
        match value {
            Match::No => return Match::No,
            Match::Unknown => unknown = true,
            _ => {}
        }
    }
    if unknown {
        Match::Unknown
    } else {
        Match::Yes
    }
}
fn any(values: impl Iterator<Item = Match>) -> Match {
    let mut unknown = false;
    for value in values {
        match value {
            Match::Yes => return Match::Yes,
            Match::Unknown => unknown = true,
            _ => {}
        }
    }
    if unknown {
        Match::Unknown
    } else {
        Match::No
    }
}
pub fn matches(version: &str, constraint: &Constraint) -> Match {
    matches_inner(version, constraint, 0)
}
fn matches_inner(version: &str, constraint: &Constraint, depth: usize) -> Match {
    if depth > 16 {
        return Match::Unknown;
    }
    match constraint {
        Constraint::Any => Match::Yes,
        Constraint::Exact(expected) => {
            if version == expected {
                Match::Yes
            } else {
                Match::No
            }
        }
        Constraint::Unknown(_) => Match::Unknown,
        Constraint::AnyOf(values) => any(values
            .iter()
            .map(|value| matches_inner(version, value, depth + 1))),
        Constraint::AllOf(values) => all(values
            .iter()
            .map(|value| matches_inner(version, value, depth + 1))),
        Constraint::Range {
            dialect,
            expression,
        } => {
            if expression.len() > 1024 || version.len() > 200 {
                return Match::Unknown;
            }
            match dialect {
                Dialect::Fabric => fabric(version, expression),
                Dialect::Maven => maven(version, expression),
            }
        }
    }
}
fn normalized(value: &str) -> Option<Version> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (main, tail) = value
        .find(['-', '+'])
        .map(|index| (&value[..index], &value[index..]))
        .unwrap_or((value, ""));
    let pieces: Vec<_> = main.split('.').collect();
    if pieces.len() > 3
        || pieces
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let mut base = pieces.join(".");
    for _ in pieces.len()..3 {
        base.push_str(".0");
    }
    Version::parse(&format!("{base}{tail}")).ok()
}
pub fn compare(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    Some(normalized(left)?.cmp_precedence(&normalized(right)?))
}
fn result(value: bool) -> Match {
    if value {
        Match::Yes
    } else {
        Match::No
    }
}
fn fabric(version: &str, expression: &str) -> Match {
    let expression = expression.trim();
    if expression.is_empty() {
        return Match::Unknown;
    }
    if expression.split("||").count() > 32 {
        return Match::Unknown;
    }
    any(expression.split("||").map(|branch| {
        let tokens: Vec<_> = branch.split_whitespace().collect();
        if tokens.is_empty() {
            return Match::Unknown;
        }
        all(tokens.into_iter().map(|token| predicate(version, token)))
    }))
}
fn predicate(version: &str, token: &str) -> Match {
    if token == "*" || token.eq_ignore_ascii_case("x") {
        return Match::Yes;
    }
    let (op, value) = [">=", "<=", ">", "<", "=", "~", "^"]
        .iter()
        .find_map(|op| token.strip_prefix(op).map(|value| (*op, value)))
        .unwrap_or(("=", token));
    if value.is_empty() {
        return Match::Unknown;
    }
    if value
        .split('.')
        .any(|part| part == "*" || part.eq_ignore_ascii_case("x"))
    {
        if op != "=" {
            return Match::Unknown;
        }
        let Some(actual) = normalized(version) else {
            return Match::Unknown;
        };
        let parts: Vec<_> = value.split('.').collect();
        if parts.len() > 3 {
            return Match::Unknown;
        }
        let numeric = [actual.major, actual.minor, actual.patch];
        let mut wildcard = false;
        for (index, part) in parts.iter().enumerate() {
            if *part == "*" || part.eq_ignore_ascii_case("x") {
                wildcard = true;
                continue;
            }
            if wildcard {
                return Match::Unknown;
            }
            let Ok(expected) = part.parse::<u64>() else {
                return Match::Unknown;
            };
            if numeric[index] != expected {
                return Match::No;
            }
        }
        return Match::Yes;
    }
    let (Some(actual), Some(expected)) = (normalized(version), normalized(value)) else {
        return if op == "=" {
            result(version == value)
        } else {
            Match::Unknown
        };
    };
    let order = actual.cmp_precedence(&expected);
    match op {
        "=" => result(order.is_eq()),
        ">=" => result(!order.is_lt()),
        ">" => result(order.is_gt()),
        "<=" => result(!order.is_gt()),
        "<" => result(order.is_lt()),
        "~" => {
            let components = value.split(['-', '+']).next().unwrap().split('.').count();
            let upper = if components == 1 {
                expected.major.checked_add(1).map(|n| Version::new(n, 0, 0))
            } else {
                expected
                    .minor
                    .checked_add(1)
                    .map(|n| Version::new(expected.major, n, 0))
            };
            upper
                .map(|upper| result(!order.is_lt() && actual.cmp_precedence(&upper).is_lt()))
                .unwrap_or(Match::Unknown)
        }
        "^" => {
            let count = value.split(['-', '+']).next().unwrap().split('.').count();
            let upper = if expected.major > 0 || count == 1 {
                expected.major.checked_add(1).map(|n| Version::new(n, 0, 0))
            } else if expected.minor > 0 || count == 2 {
                expected.minor.checked_add(1).map(|n| Version::new(0, n, 0))
            } else {
                expected.patch.checked_add(1).map(|n| Version::new(0, 0, n))
            };
            upper
                .map(|upper| result(!order.is_lt() && actual.cmp_precedence(&upper).is_lt()))
                .unwrap_or(Match::Unknown)
        }
        _ => Match::Unknown,
    }
}
fn maven(version: &str, expression: &str) -> Match {
    let expression = expression.trim();
    if expression == "*" {
        return Match::Yes;
    }
    if !expression.starts_with(['[', '(']) {
        return maven_predicate(version, "=", expression);
    }
    let mut remaining = expression;
    let mut ranges = vec![];
    while !remaining.is_empty() {
        if ranges.len() >= 32 || !remaining.starts_with(['[', '(']) {
            return Match::Unknown;
        }
        let Some(end) = remaining.find([']', ')']) else {
            return Match::Unknown;
        };
        let range = &remaining[..=end];
        let content = &range[1..range.len() - 1];
        let matched = if let Some((lower, upper)) = content.split_once(',') {
            if upper.contains(',') {
                return Match::Unknown;
            }
            let low = if lower.trim().is_empty() {
                Match::Yes
            } else {
                maven_predicate(
                    version,
                    if range.starts_with('[') { ">=" } else { ">" },
                    lower.trim(),
                )
            };
            let high = if upper.trim().is_empty() {
                Match::Yes
            } else {
                maven_predicate(
                    version,
                    if range.ends_with(']') { "<=" } else { "<" },
                    upper.trim(),
                )
            };
            all([low, high].into_iter())
        } else if range.starts_with('[') && range.ends_with(']') {
            maven_predicate(version, "=", content)
        } else {
            Match::Unknown
        };
        ranges.push(matched);
        remaining = remaining[end + 1..].trim();
        if !remaining.is_empty() {
            let Some(rest) = remaining.strip_prefix(',') else {
                return Match::Unknown;
            };
            remaining = rest.trim();
            if remaining.is_empty() {
                return Match::Unknown;
            }
        }
    }
    any(ranges.into_iter())
}

fn maven_value(value: &str) -> Option<(Vec<u64>, u8, u64)> {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern=PATTERN.get_or_init(||regex::Regex::new(r"(?i)^([0-9]+(?:\.[0-9]+)*)(?:[-.]?(alpha|a|beta|b|milestone|m|rc|cr|snapshot|final|ga|release|sp)(?:[-.]?([0-9]+))?)?$").unwrap());
    let capture = pattern.captures(value.trim())?;
    let mut numbers = capture[1]
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if numbers.len() > 8 {
        return None;
    }
    while numbers.last() == Some(&0) && numbers.len() > 1 {
        numbers.pop();
    }
    let qualifier = match capture
        .get(2)
        .map(|v| v.as_str().to_ascii_lowercase())
        .as_deref()
    {
        None | Some("final" | "ga" | "release") => 5,
        Some("alpha" | "a") => 0,
        Some("beta" | "b") => 1,
        Some("milestone" | "m") => 2,
        Some("rc" | "cr") => 3,
        Some("snapshot") => 4,
        Some("sp") => 6,
        _ => return None,
    };
    let number = capture
        .get(3)
        .map(|v| v.as_str().parse::<u64>().ok())
        .unwrap_or(Some(0))?;
    Some((numbers, qualifier, number))
}
fn maven_predicate(version: &str, op: &str, bound: &str) -> Match {
    let (Some((mut left, lq, ln)), Some((mut right, rq, rn))) =
        (maven_value(version), maven_value(bound))
    else {
        return if op == "=" && version == bound {
            Match::Yes
        } else {
            Match::Unknown
        };
    };
    let len = left.len().max(right.len());
    left.resize(len, 0);
    right.resize(len, 0);
    let order = left.cmp(&right).then(lq.cmp(&rq)).then(ln.cmp(&rn));
    match op {
        "=" => result(order.is_eq()),
        ">=" => result(!order.is_lt()),
        ">" => result(order.is_gt()),
        "<=" => result(!order.is_gt()),
        "<" => result(order.is_lt()),
        _ => Match::Unknown,
    }
}
