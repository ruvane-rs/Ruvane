use std::fmt;

/// Maximum wire bytes in one entity tag, including quotes and an optional W/ prefix.
pub const MAX_ENTITY_TAG_BYTES: usize = 128;
/// Maximum bytes in one If-Match field.
pub const MAX_IF_MATCH_BYTES: usize = 2048;
/// Maximum entity tags or ignored empty members in an If-Match list, separately.
pub const MAX_IF_MATCH_TAGS: usize = 16;

/// A conditional-header error without input fragments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityTagError {
    /// The field does not follow the ASCII entity-tag or If-Match grammar.
    InvalidSyntax,
    /// A tag or complete field exceeds its byte budget.
    TooLarge,
    /// A list exceeds its tag or empty-member budget.
    TooManyTags,
}
impl fmt::Display for EntityTagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidSyntax => "invalid entity tag syntax",
            Self::TooLarge => "conditional header exceeds byte limit",
            Self::TooManyTags => "conditional header exceeds member limit",
        })
    }
}
impl std::error::Error for EntityTagError {}

/// An immutable, bounded ASCII entity tag. Tags are public validators, not credentials.
/// A strong tag must identify exactly the selected representation's bytes.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct EntityTag(String);
impl EntityTag {
    /// Parses one quoted tag, optionally prefixed by exactly W/. No escaping or trimming occurs.
    pub fn parse(value: &str) -> Result<Self, EntityTagError> {
        if value.len() > MAX_ENTITY_TAG_BYTES {
            return Err(EntityTagError::TooLarge);
        }
        let opaque = value.strip_prefix("W/").unwrap_or(value);
        let opaque = opaque
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .ok_or(EntityTagError::InvalidSyntax)?;
        if !opaque
            .bytes()
            .all(|b| b == 0x21 || (0x23..=0x7e).contains(&b))
        {
            return Err(EntityTagError::InvalidSyntax);
        }
        Ok(Self(value.into()))
    }
    /// Constructs a strong tag from its opaque ASCII contents, without quotes.
    pub fn strong(opaque: &str) -> Result<Self, EntityTagError> {
        if opaque.len() > MAX_ENTITY_TAG_BYTES - 2 {
            return Err(EntityTagError::TooLarge);
        }
        Self::parse(&format!("\"{opaque}\""))
    }
    /// Returns the complete validated field value, including quotes.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Whether this tag is weak and cannot satisfy an If-Match comparison.
    pub fn is_weak(&self) -> bool {
        self.0.starts_with("W/")
    }
    /// Compares two strong tags exactly; weak tags never match.
    pub fn strong_eq(&self, other: &Self) -> bool {
        !self.is_weak() && !other.is_weak() && self == other
    }
}
impl fmt::Debug for EntityTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EntityTag([redacted])")
    }
}

#[derive(Clone, Eq, PartialEq)]
enum Condition {
    Any,
    Tags(Vec<EntityTag>),
}
/// A bounded If-Match condition. Evaluate against current state while holding the mutation lock.
#[derive(Clone, Eq, PartialEq)]
pub struct IfMatch(Condition);
impl IfMatch {
    /// Parses wildcard existence or a comma-separated tag list with optional spaces/tabs.
    /// Up to sixteen empty list members are ignored. Backslash is literal inside tags.
    pub fn parse(value: &str) -> Result<Self, EntityTagError> {
        if value.len() > MAX_IF_MATCH_BYTES {
            return Err(EntityTagError::TooLarge);
        }
        let value = value.trim_matches([' ', '\t']);
        if value == "*" {
            return Ok(Self(Condition::Any));
        }
        let (mut tags, mut rest, mut empty) = (Vec::new(), value, 0);
        loop {
            rest = rest.trim_start_matches([' ', '\t']);
            if rest.is_empty() || rest.starts_with(',') {
                empty += 1;
                if empty > MAX_IF_MATCH_TAGS {
                    return Err(EntityTagError::TooManyTags);
                }
            } else {
                let quoted = rest.strip_prefix("W/").unwrap_or(rest);
                if !quoted.starts_with('"') {
                    return Err(EntityTagError::InvalidSyntax);
                }
                let end = quoted[1..].find('"').ok_or(EntityTagError::InvalidSyntax)?
                    + 2
                    + (rest.len() - quoted.len());
                if tags.len() == MAX_IF_MATCH_TAGS {
                    return Err(EntityTagError::TooManyTags);
                }
                tags.push(EntityTag::parse(&rest[..end])?);
                rest = &rest[end..];
            }
            rest = rest.trim_start_matches([' ', '\t']);
            if rest.is_empty() {
                break;
            }
            rest = rest
                .strip_prefix(',')
                .ok_or(EntityTagError::InvalidSyntax)?;
        }
        Ok(Self(Condition::Tags(tags)))
    }
    /// Whether this condition only requires the resource to exist, without checking a version.
    pub fn is_any(&self) -> bool {
        matches!(self.0, Condition::Any)
    }
    /// Evaluates existence or strong equality. Missing representations never match.
    pub fn matches(&self, current: Option<&EntityTag>) -> bool {
        let Some(current) = current else {
            return false;
        };
        match &self.0 {
            Condition::Any => true,
            Condition::Tags(tags) => tags.iter().any(|tag| tag.strong_eq(current)),
        }
    }
}
impl fmt::Debug for IfMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IfMatch([redacted])")
    }
}

#[cfg(not(target_family = "wasm"))]
mod source;
#[cfg(not(target_family = "wasm"))]
pub use source::{EntityTagSource, EntityTagSourceError};
#[cfg(test)]
mod tests;
