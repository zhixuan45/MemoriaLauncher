//! Bounded SNBT reader/writer for FTB chapter editing. Unknown fields and numeric types survive.
use super::{invalid, Result};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Compound(BTreeMap<String, Node>),
    List(Vec<Node>),
    String(String),
    Number(String),
    Array(char, Vec<Node>),
}
impl Node {
    pub fn object(&self) -> Result<&BTreeMap<String, Node>> {
        if let Self::Compound(v) = self {
            Ok(v)
        } else {
            Err(invalid("SNBT 节点不是 compound"))
        }
    }
    pub fn object_mut(&mut self) -> Result<&mut BTreeMap<String, Node>> {
        if let Self::Compound(v) = self {
            Ok(v)
        } else {
            Err(invalid("SNBT 节点不是 compound"))
        }
    }
    pub fn list(&self) -> Result<&Vec<Node>> {
        if let Self::List(v) = self {
            Ok(v)
        } else {
            Err(invalid("SNBT 节点不是 list"))
        }
    }
    pub fn list_mut(&mut self) -> Result<&mut Vec<Node>> {
        if let Self::List(v) = self {
            Ok(v)
        } else {
            Err(invalid("SNBT 节点不是 list"))
        }
    }
    pub fn text(&self) -> Option<&str> {
        if let Self::String(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }
    pub fn number(value: impl ToString) -> Self {
        Self::Number(value.to_string())
    }
    pub fn empty() -> Self {
        Self::Compound(BTreeMap::new())
    }
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.object().ok()?.get(key)
    }
    pub fn render(&self) -> String {
        let mut out = String::new();
        render(self, 0, &mut out);
        out.push('\n');
        out
    }
}
fn render(node: &Node, depth: usize, out: &mut String) {
    match node {
        Node::String(v) => out.push_str(&serde_json::to_string(v).expect("string")),
        Node::Number(v) => out.push_str(v),
        Node::List(values) | Node::Array(_, values) => {
            out.push('[');
            if let Node::Array(kind, _) = node {
                out.push(*kind);
                out.push(';');
            }
            for (i, value) in values.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                out.push_str(&"  ".repeat(depth + 1));
                render(value, depth + 1, out);
            }
            if !values.is_empty() {
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
            }
            out.push(']');
        }
        Node::Compound(values) => {
            out.push('{');
            for (i, (key, value)) in values.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                out.push_str(&"  ".repeat(depth + 1));
                out.push_str(&serde_json::to_string(key).expect("key"));
                out.push_str(": ");
                render(value, depth + 1, out);
            }
            if !values.is_empty() {
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
            }
            out.push('}');
        }
    }
}
pub fn parse(text: &str) -> Result<Node> {
    if text.len() > super::files::MAX_TEXT_BYTES {
        return Err(invalid("SNBT 文件超过 1 MiB"));
    }
    let mut parser = Parser {
        text,
        offset: 0,
        nodes: 0,
    };
    let node = parser.node(0)?;
    parser.space()?;
    if parser.offset != text.len() {
        return Err(invalid("SNBT 尾部存在多余内容"));
    }
    node.object()?;
    Ok(node)
}
struct Parser<'a> {
    text: &'a str,
    offset: usize,
    nodes: usize,
}
impl Parser<'_> {
    fn rest(&self) -> &str {
        &self.text[self.offset..]
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn pop(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.offset += c.len_utf8();
        Some(c)
    }
    fn space(&mut self) -> Result<()> {
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                self.pop();
            }
            if self.rest().starts_with("//") || self.rest().starts_with('#') {
                while self.pop().is_some_and(|c| c != '\n') {}
            } else if self.rest().starts_with("/*") {
                let end = self
                    .rest()
                    .find("*/")
                    .ok_or_else(|| invalid("SNBT 注释未结束"))?;
                self.offset += end + 2;
            } else {
                return Ok(());
            }
        }
    }
    fn expect(&mut self, c: char) -> Result<()> {
        self.space()?;
        if self.pop() == Some(c) {
            Ok(())
        } else {
            Err(invalid(format!("SNBT 预期字符 {c}")))
        }
    }
    fn string(&mut self) -> Result<String> {
        let quote = self.pop().ok_or_else(|| invalid("SNBT 字符串缺失"))?;
        let mut value = String::new();
        while let Some(c) = self.pop() {
            if c == quote {
                return Ok(value);
            }
            if c == '\\' {
                let escaped = self.pop().ok_or_else(|| invalid("SNBT 转义未结束"))?;
                match escaped {
                    '\\' | '\'' | '"' => value.push(escaped),
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    'b' => value.push('\u{8}'),
                    'f' => value.push('\u{c}'),
                    'u' => {
                        let start = self.offset;
                        for _ in 0..4 {
                            if !self.pop().is_some_and(|c| c.is_ascii_hexdigit()) {
                                return Err(invalid("SNBT Unicode 转义无效"));
                            }
                        }
                        let n = u32::from_str_radix(&self.text[start..self.offset], 16)
                            .map_err(invalid)?;
                        value.push(
                            char::from_u32(n)
                                .ok_or_else(|| invalid("SNBT Unicode 转义不受支持"))?,
                        );
                    }
                    _ => return Err(invalid("SNBT 转义无效")),
                }
            } else {
                value.push(c);
            }
            if value.len() > 64 * 1024 {
                return Err(invalid("SNBT 字符串过长"));
            }
        }
        Err(invalid("SNBT 字符串未结束"))
    }
    fn token(&mut self, key: bool) -> Result<String> {
        let start = self.offset;
        while self
            .peek()
            .is_some_and(|c| !c.is_whitespace() && !",]}{[".contains(c) && !(key && c == ':'))
        {
            self.pop();
        }
        if self.offset == start || self.offset - start > 1024 {
            return Err(invalid("SNBT token 无效"));
        }
        Ok(self.text[start..self.offset].to_string())
    }
    fn node(&mut self, depth: usize) -> Result<Node> {
        self.nodes += 1;
        if depth > 48 || self.nodes > 50_000 {
            return Err(invalid("SNBT 结构过深或节点过多"));
        }
        self.space()?;
        match self.peek() {
            Some('{') => {
                self.pop();
                let mut object = BTreeMap::new();
                loop {
                    self.space()?;
                    if self.peek() == Some('}') {
                        self.pop();
                        break;
                    }
                    let key = if matches!(self.peek(), Some('"' | '\'')) {
                        self.string()?
                    } else {
                        self.token(true)?
                    };
                    self.expect(':')?;
                    let value = self.node(depth + 1)?;
                    if object.insert(key, value).is_some() {
                        return Err(invalid("SNBT compound 含重复字段"));
                    }
                    self.space()?;
                    if self.peek() == Some(',') {
                        self.pop();
                    }
                    if self.peek().is_none() {
                        return Err(invalid("SNBT compound 未结束"));
                    }
                }
                Ok(Node::Compound(object))
            }
            Some('[') => {
                self.pop();
                self.space()?;
                let typed = if matches!(self.peek(), Some('B' | 'I' | 'L'))
                    && self.rest().as_bytes().get(1) == Some(&b';')
                {
                    let c = self.pop();
                    self.pop();
                    c
                } else {
                    None
                };
                let mut list = vec![];
                loop {
                    self.space()?;
                    if self.peek() == Some(']') {
                        self.pop();
                        break;
                    }
                    list.push(self.node(depth + 1)?);
                    self.space()?;
                    if self.peek() == Some(',') {
                        self.pop();
                    }
                    if self.peek().is_none() {
                        return Err(invalid("SNBT list 未结束"));
                    }
                }
                Ok(if let Some(kind) = typed {
                    if list.iter().any(|v| !matches!(v, Node::Number(_))) {
                        return Err(invalid("SNBT 数组元素不是数值"));
                    }
                    Node::Array(kind, list)
                } else {
                    Node::List(list)
                })
            }
            Some('"' | '\'') => self.string().map(Node::String),
            Some(_) => {
                let value = self.token(false)?;
                let number_pattern = {
                    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
                    PATTERN.get_or_init(|| regex::Regex::new(r"(?i)^[+-]?(?:[0-9]+[bsl]|(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:e[+-]?[0-9]+)?[fd]?)$").expect("numeric pattern"))
                };
                let numeric = value
                    .strip_suffix(['b', 'B', 's', 'S', 'l', 'L', 'f', 'F', 'd', 'D'])
                    .unwrap_or(&value);
                if matches!(value.as_str(), "true" | "false")
                    || (number_pattern.is_match(&value)
                        && numeric.parse::<f64>().is_ok_and(f64::is_finite))
                {
                    Ok(Node::Number(value))
                } else {
                    Ok(Node::String(value))
                }
            }
            None => Err(invalid("SNBT 内容缺失")),
        }
    }
}
