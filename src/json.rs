use std::fmt;

/// Fixed MIME type of validated UTF-8 JSON responses.
pub const JSON_MEDIA_TYPE: &str = "application/json";
/// Maximum input or encoded output bytes in one JSON value.
pub const MAX_JSON_BYTES: usize = 64 * 1024;
/// Maximum nested array/object containers, including the root container.
pub const MAX_JSON_DEPTH: usize = 16;
/// Maximum values, including containers and the root. Object keys are not values.
pub const MAX_JSON_VALUES: usize = 1024;
/// Maximum members in one object or elements in one array.
pub const MAX_JSON_ITEMS: usize = 128;
/// Maximum decoded UTF-8 bytes in one string or object key.
pub const MAX_JSON_STRING_BYTES: usize = 4096;
/// Maximum ASCII bytes in one number's preserved representation.
pub const MAX_JSON_NUMBER_BYTES: usize = 128;

/// JSON errors contain no input, key, number or string values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonError {
    /// Input, output, a string or a number exceeds its byte budget.
    TooLarge,
    /// Too many nested containers.
    TooDeep,
    /// Too many values in one tree.
    TooManyValues,
    /// Too many members or elements in one container.
    TooManyItems,
    /// Input bytes are not valid UTF-8.
    InvalidUtf8,
    /// Malformed grammar, trailing data or unsupported extensions.
    InvalidSyntax,
    /// Invalid escapes, unescaped controls or unpaired UTF-16 surrogates.
    InvalidString,
    /// A number does not follow the JSON number grammar.
    InvalidNumber,
    /// Decoded object keys are repeated.
    DuplicateKey,
    /// A number cannot be converted to the requested Rust numeric type.
    NumberConversion,
}
impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "JSON byte limit exceeded",
            Self::TooDeep => "JSON depth limit exceeded",
            Self::TooManyValues => "JSON value count limit exceeded",
            Self::TooManyItems => "JSON container count limit exceeded",
            Self::InvalidUtf8 => "invalid JSON UTF-8",
            Self::InvalidSyntax => "invalid JSON syntax",
            Self::InvalidString => "invalid JSON string",
            Self::InvalidNumber => "invalid JSON number",
            Self::DuplicateKey => "duplicate JSON key",
            Self::NumberConversion => "JSON number conversion failed",
        })
    }
}
impl std::error::Error for JsonError {}
impl JsonError {
    pub(crate) fn is_limit(self) -> bool {
        matches!(
            self,
            Self::TooLarge | Self::TooDeep | Self::TooManyValues | Self::TooManyItems
        )
    }
}

/// Validated number text, preserved without an implicit floating-point conversion.
/// Debug redacts the representation. Equality compares representations, not numeric equivalence.
#[derive(Clone, Eq, PartialEq)]
pub struct JsonNumber(String);
impl fmt::Debug for JsonNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JsonNumber([redacted])")
    }
}
impl JsonNumber {
    /// Parses one complete bounded JSON number. Whitespace is not part of the number.
    pub fn parse(value: &str) -> Result<Self, JsonError> {
        if value.len() > MAX_JSON_NUMBER_BYTES {
            return Err(JsonError::TooLarge);
        }
        if number_end(value.as_bytes(), 0)? != value.len() {
            return Err(JsonError::InvalidNumber);
        }
        Ok(Self(value.into()))
    }
    /// Exposes the exact validated number representation for intentional use.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Converts an integer literal to i64, rejecting fractions, exponents and overflow.
    pub fn to_i64(&self) -> Result<i64, JsonError> {
        self.0.parse().map_err(|_| JsonError::NumberConversion)
    }
    /// Converts an unsigned integer literal to u64, rejecting signs, fractions, exponents and overflow.
    pub fn to_u64(&self) -> Result<u64, JsonError> {
        if self.0.starts_with('-') {
            return Err(JsonError::NumberConversion);
        }
        self.0.parse().map_err(|_| JsonError::NumberConversion)
    }
    /// Explicit IEEE-754 conversion. May round or underflow; infinity and NaN are rejected.
    pub fn to_f64(&self) -> Result<f64, JsonError> {
        let value: f64 = self.0.parse().map_err(|_| JsonError::NumberConversion)?;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(JsonError::NumberConversion)
        }
    }
}
impl From<i64> for JsonNumber {
    fn from(value: i64) -> Self {
        Self(value.to_string())
    }
}
impl From<u64> for JsonNumber {
    fn from(value: u64) -> Self {
        Self(value.to_string())
    }
}

#[derive(Clone, Eq, PartialEq)]
enum Kind {
    Null,
    Boolean(bool),
    Number(JsonNumber),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}
/// An immutable, validated JSON tree. Constructors enforce the same profile as parsing.
/// Objects retain insertion order. Debug omits keys and values; encoded output is sensitive data.
#[derive(Clone, Eq, PartialEq)]
pub struct JsonValue {
    kind: Kind,
    depth: usize,
    values: usize,
    bytes: usize,
}
impl fmt::Debug for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsonValue")
            .field("depth", &self.depth)
            .field("values", &self.values)
            .field("encoded_bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}
impl Default for JsonValue {
    fn default() -> Self {
        Self::null()
    }
}
impl JsonValue {
    /// Creates JSON null.
    pub fn null() -> Self {
        Self {
            kind: Kind::Null,
            depth: 0,
            values: 1,
            bytes: 4,
        }
    }
    /// Creates a bounded Unicode string. Escaped controls are valid JSON data.
    pub fn string(value: impl Into<String>) -> Result<Self, JsonError> {
        let value = value.into();
        let bytes = string_size(&value)?;
        Ok(Self {
            kind: Kind::String(value),
            depth: 0,
            values: 1,
            bytes,
        })
    }
    /// Creates an array after checking item, depth, value and encoded-byte budgets.
    pub fn array(values: Vec<Self>) -> Result<Self, JsonError> {
        Self::container(Kind::Array(values))
    }
    /// Creates an ordered object with unique decoded keys and bounded members.
    pub fn object(values: Vec<(String, Self)>) -> Result<Self, JsonError> {
        if values.len() > MAX_JSON_ITEMS {
            return Err(JsonError::TooManyItems);
        }
        for (index, (key, _)) in values.iter().enumerate() {
            string_size(key)?;
            if values[..index].iter().any(|(previous, _)| previous == key) {
                return Err(JsonError::DuplicateKey);
            }
        }
        Self::container(Kind::Object(values))
    }
    fn container(kind: Kind) -> Result<Self, JsonError> {
        let mut depth = 1;
        let mut values = 1;
        let mut bytes = 2;
        let children: Vec<(&Self, usize)> = match &kind {
            Kind::Array(items) => {
                if items.len() > MAX_JSON_ITEMS {
                    return Err(JsonError::TooManyItems);
                }
                items.iter().map(|value| (value, 0)).collect()
            }
            Kind::Object(items) => items
                .iter()
                .map(|(key, value)| Ok((value, string_size(key)? + 1)))
                .collect::<Result<_, JsonError>>()?,
            _ => unreachable!(),
        };
        for (index, (child, extra)) in children.into_iter().enumerate() {
            depth = depth.max(child.depth + 1);
            values += child.values;
            bytes += child.bytes + extra + usize::from(index != 0);
            if depth > MAX_JSON_DEPTH {
                return Err(JsonError::TooDeep);
            }
            if values > MAX_JSON_VALUES {
                return Err(JsonError::TooManyValues);
            }
            if bytes > MAX_JSON_BYTES {
                return Err(JsonError::TooLarge);
            }
        }
        Ok(Self {
            kind,
            depth,
            values,
            bytes,
        })
    }
    /// Parses one complete UTF-8 JSON value, with no BOM, extensions or trailing data.
    pub fn parse(bytes: &[u8]) -> Result<Self, JsonError> {
        if bytes.len() > MAX_JSON_BYTES {
            return Err(JsonError::TooLarge);
        }
        let input = std::str::from_utf8(bytes).map_err(|_| JsonError::InvalidUtf8)?;
        let mut parser = Parser {
            input,
            pos: 0,
            values: 0,
        };
        let value = parser.value(0)?;
        parser.whitespace();
        if parser.pos != input.len() {
            return Err(JsonError::InvalidSyntax);
        }
        Ok(value)
    }
    /// Returns compact, bounded UTF-8 JSON. Number representations are preserved exactly.
    /// This is a JSON document, not an HTML or JavaScript embedding API.
    pub fn encode(&self) -> String {
        let mut output = String::with_capacity(self.bytes);
        self.write(&mut output);
        debug_assert_eq!(output.len(), self.bytes);
        output
    }
    fn write(&self, output: &mut String) {
        match &self.kind {
            Kind::Null => output.push_str("null"),
            Kind::Boolean(value) => output.push_str(if *value { "true" } else { "false" }),
            Kind::Number(value) => output.push_str(value.as_str()),
            Kind::String(value) => write_string(output, value),
            Kind::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    value.write(output);
                }
                output.push(']');
            }
            Kind::Object(values) => {
                output.push('{');
                for (index, (key, value)) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    write_string(output, key);
                    output.push(':');
                    value.write(output);
                }
                output.push('}');
            }
        }
    }
    /// Looks up one exact decoded object key. Arrays and scalars return None.
    pub fn get(&self, name: &str) -> Option<&Self> {
        self.as_object()?
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }
    /// Borrows ordered object members without granting mutable access.
    pub fn as_object(&self) -> Option<&[(String, Self)]> {
        if let Kind::Object(value) = &self.kind {
            Some(value)
        } else {
            None
        }
    }
    /// Borrows array elements without granting mutable access.
    pub fn as_array(&self) -> Option<&[Self]> {
        if let Kind::Array(value) = &self.kind {
            Some(value)
        } else {
            None
        }
    }
    /// Borrows decoded Unicode string data.
    pub fn as_str(&self) -> Option<&str> {
        if let Kind::String(value) = &self.kind {
            Some(value)
        } else {
            None
        }
    }
    /// Borrows exact number data. No implicit numeric conversion occurs.
    pub fn as_number(&self) -> Option<&JsonNumber> {
        if let Kind::Number(value) = &self.kind {
            Some(value)
        } else {
            None
        }
    }
    /// Reads a boolean without coercion.
    pub fn as_bool(&self) -> Option<bool> {
        if let Kind::Boolean(value) = &self.kind {
            Some(*value)
        } else {
            None
        }
    }
    /// Returns whether this value is JSON null.
    pub fn is_null(&self) -> bool {
        matches!(self.kind, Kind::Null)
    }
}
impl From<bool> for JsonValue {
    fn from(value: bool) -> Self {
        Self {
            kind: Kind::Boolean(value),
            depth: 0,
            values: 1,
            bytes: if value { 4 } else { 5 },
        }
    }
}
impl From<JsonNumber> for JsonValue {
    fn from(value: JsonNumber) -> Self {
        let bytes = value.0.len();
        Self {
            kind: Kind::Number(value),
            depth: 0,
            values: 1,
            bytes,
        }
    }
}
impl From<i64> for JsonValue {
    fn from(value: i64) -> Self {
        JsonNumber::from(value).into()
    }
}
impl From<u64> for JsonValue {
    fn from(value: u64) -> Self {
        JsonNumber::from(value).into()
    }
}

fn string_size(value: &str) -> Result<usize, JsonError> {
    if value.len() > MAX_JSON_STRING_BYTES {
        return Err(JsonError::TooLarge);
    }
    Ok(2 + value
        .chars()
        .map(|ch| match ch {
            '"' | '\\' | '\u{8}' | '\u{c}' | '\n' | '\r' | '\t' => 2,
            '\0'..='\u{1f}' => 6,
            _ => ch.len_utf8(),
        })
        .sum::<usize>())
}
fn write_string(output: &mut String, value: &str) {
    const HEX: &[u8] = b"0123456789abcdef";
    output.push('"');
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{8}' => output.push_str("\\b"),
            '\u{c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\0'..='\u{1f}' => {
                output.push_str("\\u00");
                output.push(HEX[ch as usize >> 4] as char);
                output.push(HEX[ch as usize & 15] as char);
            }
            _ => output.push(ch),
        }
    }
    output.push('"');
}
fn number_end(input: &[u8], mut pos: usize) -> Result<usize, JsonError> {
    if input.get(pos) == Some(&b'-') {
        pos += 1;
    }
    match input.get(pos) {
        Some(b'0') => pos += 1,
        Some(b'1'..=b'9') => {
            pos += 1;
            while input.get(pos).is_some_and(u8::is_ascii_digit) {
                pos += 1;
            }
        }
        _ => return Err(JsonError::InvalidNumber),
    }
    if input.get(pos) == Some(&b'.') {
        pos += 1;
        let start = pos;
        while input.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1;
        }
        if start == pos {
            return Err(JsonError::InvalidNumber);
        }
    }
    if matches!(input.get(pos), Some(b'e' | b'E')) {
        pos += 1;
        if matches!(input.get(pos), Some(b'+' | b'-')) {
            pos += 1;
        }
        let start = pos;
        while input.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1;
        }
        if start == pos {
            return Err(JsonError::InvalidNumber);
        }
    }
    Ok(pos)
}
struct Parser<'a> {
    input: &'a str,
    pos: usize,
    values: usize,
}
impl Parser<'_> {
    fn byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }
    fn whitespace(&mut self) {
        while matches!(self.byte(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }
    fn take(&mut self, expected: u8) -> Result<(), JsonError> {
        if self.byte() != Some(expected) {
            return Err(JsonError::InvalidSyntax);
        }
        self.pos += 1;
        Ok(())
    }
    fn value(&mut self, depth: usize) -> Result<JsonValue, JsonError> {
        self.whitespace();
        self.values += 1;
        if self.values > MAX_JSON_VALUES {
            return Err(JsonError::TooManyValues);
        }
        match self.byte() {
            Some(b'"') => JsonValue::string(self.string()?),
            Some(b'[' | b'{') => self.container(depth + 1),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.pos;
                self.pos = number_end(self.input.as_bytes(), start)?;
                Ok(JsonNumber::parse(&self.input[start..self.pos])?.into())
            }
            Some(b'n') => {
                self.literal("null")?;
                Ok(JsonValue::null())
            }
            Some(b't') => {
                self.literal("true")?;
                Ok(true.into())
            }
            Some(b'f') => {
                self.literal("false")?;
                Ok(false.into())
            }
            _ => Err(JsonError::InvalidSyntax),
        }
    }
    fn literal(&mut self, text: &str) -> Result<(), JsonError> {
        if !self.input[self.pos..].starts_with(text) {
            return Err(JsonError::InvalidSyntax);
        }
        self.pos += text.len();
        Ok(())
    }
    fn container(&mut self, depth: usize) -> Result<JsonValue, JsonError> {
        if depth > MAX_JSON_DEPTH {
            return Err(JsonError::TooDeep);
        }
        let object = self.byte() == Some(b'{');
        self.pos += 1;
        self.whitespace();
        let close = if object { b'}' } else { b']' };
        let mut values = Vec::new();
        let mut members: Vec<(String, JsonValue)> = Vec::new();
        if self.byte() == Some(close) {
            self.pos += 1;
            return if object {
                JsonValue::object(members)
            } else {
                JsonValue::array(values)
            };
        }
        loop {
            if values.len() + members.len() == MAX_JSON_ITEMS {
                return Err(JsonError::TooManyItems);
            }
            self.whitespace();
            if object {
                let key = self.string()?;
                if members.iter().any(|(previous, _)| previous == &key) {
                    return Err(JsonError::DuplicateKey);
                }
                self.whitespace();
                self.take(b':')?;
                let value = self.value(depth)?;
                members.push((key, value));
            } else {
                values.push(self.value(depth)?);
            }
            self.whitespace();
            if self.byte() == Some(close) {
                self.pos += 1;
                break;
            }
            self.take(b',')?;
        }
        if object {
            JsonValue::object(members)
        } else {
            JsonValue::array(values)
        }
    }
    fn hex(&mut self) -> Result<u32, JsonError> {
        let mut value = 0;
        for _ in 0..4 {
            let ch = self.byte().ok_or(JsonError::InvalidString)?;
            value = value * 16 + (ch as char).to_digit(16).ok_or(JsonError::InvalidString)?;
            self.pos += 1;
        }
        Ok(value)
    }
    fn string(&mut self) -> Result<String, JsonError> {
        self.take(b'"')?;
        let mut value = String::new();
        loop {
            let ch = match self.byte().ok_or(JsonError::InvalidString)? {
                b'"' => {
                    self.pos += 1;
                    return Ok(value);
                }
                b'\\' => {
                    self.pos += 1;
                    let escape = self.byte().ok_or(JsonError::InvalidString)?;
                    self.pos += 1;
                    match escape {
                        b'"' | b'\\' | b'/' => escape as char,
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut scalar = self.hex()?;
                            if (0xd800..=0xdbff).contains(&scalar) {
                                if !self.input[self.pos..].starts_with("\\u") {
                                    return Err(JsonError::InvalidString);
                                }
                                self.pos += 2;
                                let low = self.hex()?;
                                if !(0xdc00..=0xdfff).contains(&low) {
                                    return Err(JsonError::InvalidString);
                                }
                                scalar = 0x10000 + ((scalar - 0xd800) << 10) + low - 0xdc00;
                            }
                            char::from_u32(scalar).ok_or(JsonError::InvalidString)?
                        }
                        _ => return Err(JsonError::InvalidString),
                    }
                }
                0..=31 => return Err(JsonError::InvalidString),
                _ => {
                    let ch = self.input[self.pos..]
                        .chars()
                        .next()
                        .ok_or(JsonError::InvalidString)?;
                    self.pos += ch.len_utf8();
                    ch
                }
            };
            if value.len() + ch.len_utf8() > MAX_JSON_STRING_BYTES {
                return Err(JsonError::TooLarge);
            }
            value.push(ch);
        }
    }
}

#[cfg(test)]
mod tests;
