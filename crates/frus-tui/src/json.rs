// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The JSON lines the workbench reads from `frus daemon` and `frus run --machine`, and the
//! commands it writes to them: the little of JSON a line holds, without a dependency.

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    /// A member of an object.
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// A string member, or `None` when it is missing or not a string.
    pub(crate) fn str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    /// A boolean member, `false` when it is missing.
    pub(crate) fn flag(&self, key: &str) -> bool {
        self.get(key).and_then(Value::as_bool).unwrap_or(false)
    }
}

/// `text` as a JSON string, quotes included.
pub(crate) fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Reads one JSON value, the whole of `text`.
pub(crate) fn parse(text: &str) -> Result<Value, String> {
    let mut reader = Reader {
        chars: text.trim_start_matches('\u{feff}').chars().collect(),
        at: 0,
    };
    let value = reader.value()?;
    reader.blank();
    if reader.at != reader.chars.len() {
        return Err("text after the value".into());
    }
    Ok(value)
}

struct Reader {
    chars: Vec<char>,
    at: usize,
}

impl Reader {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek();
        self.at += 1;
        c
    }

    fn blank(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    fn expect(&mut self, word: &str) -> Result<(), String> {
        for wanted in word.chars() {
            if self.next() != Some(wanted) {
                return Err(format!("expected `{word}`"));
            }
        }
        Ok(())
    }

    fn value(&mut self) -> Result<Value, String> {
        self.blank();
        match self.peek().ok_or("the line ends too soon")? {
            'n' => self.expect("null").map(|()| Value::Null),
            't' => self.expect("true").map(|()| Value::Bool(true)),
            'f' => self.expect("false").map(|()| Value::Bool(false)),
            '"' => self.string().map(Value::String),
            '[' => {
                self.at += 1;
                let mut items = Vec::new();
                self.blank();
                if self.peek() == Some(']') {
                    self.at += 1;
                    return Ok(Value::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    self.blank();
                    match self.next() {
                        Some(',') => continue,
                        Some(']') => return Ok(Value::Array(items)),
                        _ => return Err("expected `,` or `]`".into()),
                    }
                }
            }
            '{' => {
                self.at += 1;
                let mut members = Vec::new();
                self.blank();
                if self.peek() == Some('}') {
                    self.at += 1;
                    return Ok(Value::Object(members));
                }
                loop {
                    self.blank();
                    let key = self.string()?;
                    self.blank();
                    if self.next() != Some(':') {
                        return Err("expected `:`".into());
                    }
                    members.push((key, self.value()?));
                    self.blank();
                    match self.next() {
                        Some(',') => continue,
                        Some('}') => return Ok(Value::Object(members)),
                        _ => return Err("expected `,` or `}`".into()),
                    }
                }
            }
            c if c == '-' || c.is_ascii_digit() => {
                let start = self.at;
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'))
                {
                    self.at += 1;
                }
                let text: String = self.chars[start..self.at].iter().collect();
                text.parse()
                    .map(Value::Number)
                    .map_err(|_| format!("not a number: {text}"))
            }
            c => Err(format!("unexpected `{c}`")),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if self.next() != Some('"') {
            return Err("expected a string".into());
        }
        let mut out = String::new();
        loop {
            match self.next().ok_or("a string is not closed")? {
                '"' => return Ok(out),
                '\\' => match self.next().ok_or("a string is not closed")? {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let high = self.hex4()?;
                        let code = if (0xd800..0xdc00).contains(&high)
                            && self.chars.get(self.at) == Some(&'\\')
                            && self.chars.get(self.at + 1) == Some(&'u')
                        {
                            self.at += 2;
                            let low = self.hex4()?;
                            0x10000 + ((high - 0xd800) << 10) + (low.wrapping_sub(0xdc00) & 0x3ff)
                        } else {
                            high
                        };
                        out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    }
                    other => out.push(other),
                },
                c => out.push(c),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut code = 0;
        for _ in 0..4 {
            let digit = self
                .next()
                .and_then(|c| c.to_digit(16))
                .ok_or("a bad \\u escape")?;
            code = code * 16 + digit;
        }
        Ok(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_daemon_line_is_read() {
        let line = r#"{"id":1,"result":[{"id":"windows","name":"This \"PC\"","available":true,"unavailableReason":null,"n":-1.5e2}]}"#;
        let value = parse(line).unwrap();
        let device = &value.get("result").unwrap().as_array().unwrap()[0];
        assert_eq!(device.str("name"), Some("This \"PC\""));
        assert!(device.flag("available"));
        assert_eq!(device.get("unavailableReason"), Some(&Value::Null));
        assert_eq!(device.get("n").and_then(Value::as_f64), Some(-150.0));
    }

    #[test]
    fn escapes_go_both_ways() {
        let text = "a\tb\n\"c\" \\ é \u{1}";
        assert_eq!(parse(&quote(text)).unwrap(), Value::String(text.into()));
        assert_eq!(parse(r#""é😀""#).unwrap(), Value::String("é😀".into()));
    }

    #[test]
    fn broken_lines_are_refused() {
        for line in [
            "",
            "{",
            "{\"a\" 1}",
            "[1,]",
            "\"open",
            "{} x",
            "Compiling foo",
        ] {
            assert!(parse(line).is_err(), "{line}");
        }
        // PowerShell's byte-order mark is not part of the line.
        assert_eq!(parse("\u{feff}true").unwrap(), Value::Bool(true));
    }
}
