use crate::app::AppError;
use crate::routing::{Segment, pattern_segments};
use crate::url::{encode, valid_segment};
use crate::{InternalUrl, MAX_INTERNAL_URL_BYTES, MAX_PATH_PARAMS};

/// Maximum ASCII bytes in a page route or layout name.
pub const MAX_PAGE_NAME_BYTES: usize = 48;
/// Maximum segments in a page pattern, excluding the final directory slash.
pub const MAX_PAGE_SEGMENTS: usize = 16;

pub(crate) fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PAGE_NAME_BYTES
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

/// A named page pattern used for registration and checked URL construction.
///
/// Captures occupy complete segments, as in `Router`. Names are ASCII identifiers;
/// literal segments also pass `InternalUrl` validation. Patterns have at most
/// 16 segments and 4096 bytes. This value alone does not prove registration:
/// use `App::url` or `PageContext::url` to also check the application's namespace.
#[derive(Clone, Debug)]
pub struct PageRoute {
    name: String,
    pattern: String,
    pub(crate) segments: Vec<Segment>,
    pub(crate) dynamic: bool,
}

impl PageRoute {
    /// Validates a route name and pattern without executing application code.
    ///
    /// ```
    /// let article = ruvane::PageRoute::new("article", "/articles/{slug}/").unwrap();
    /// assert_eq!(article.url(&[("slug", "été & Rust")]).unwrap().as_str(),
    ///            "/articles/%C3%A9t%C3%A9%20%26%20Rust/");
    /// assert!(article.url(&[]).is_err());
    /// ```
    pub fn new(name: &str, pattern: &str) -> Result<Self, AppError> {
        if !valid_name(name) {
            return Err(AppError::InvalidName);
        }
        if pattern.len() > MAX_INTERNAL_URL_BYTES {
            return Err(AppError::InvalidParams);
        }
        let (segments, dynamic) = pattern_segments(pattern)?;
        let count = segments.len() - usize::from(pattern.ends_with('/'));
        if count > MAX_PAGE_SEGMENTS {
            return Err(AppError::InvalidParams);
        }
        for segment in &segments {
            if let Segment::Literal(value) = segment {
                let _ = InternalUrl::new(&format!("/{value}"))?;
            }
        }
        Ok(Self {
            name: name.into(),
            pattern: pattern.into(),
            segments,
            dynamic,
        })
    }

    /// Returns the registered name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the validated, raw pattern.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Builds an internal URL from exactly one decoded value per capture.
    /// Values are encoded once; duplicate, missing and extra names are errors.
    /// Separators, dot segments, controls and oversized values are refused.
    /// Append query values to the resulting `InternalUrl` explicitly.
    pub fn url(&self, params: &[(&str, &str)]) -> Result<InternalUrl, AppError> {
        if params.len() > MAX_PATH_PARAMS
            || params.len()
                != self
                    .segments
                    .iter()
                    .filter(|s| matches!(s, Segment::Capture(_)))
                    .count()
        {
            return Err(AppError::InvalidParams);
        }
        for (index, (key, _)) in params.iter().enumerate() {
            if params[..index].iter().any(|(other, _)| other == key) {
                return Err(AppError::InvalidParams);
            }
        }
        let mut path = String::new();
        for segment in &self.segments {
            let value = match segment {
                Segment::Literal(value) => value.clone(),
                Segment::Capture(name) => {
                    let value = params
                        .iter()
                        .find(|(key, _)| *key == name)
                        .ok_or(AppError::InvalidParams)?
                        .1;
                    valid_segment(value)?;
                    encode(value)
                }
            };
            if path.len() + value.len() + 1 > MAX_INTERNAL_URL_BYTES {
                return Err(AppError::InvalidParams);
            }
            path.push('/');
            path.push_str(&value);
        }
        Ok(InternalUrl::new(&path)?)
    }

    pub(crate) fn matches(&self, path: &str) -> bool {
        let Some(path) = path.strip_prefix('/') else {
            return false;
        };
        let mut parts = path.split('/');
        for segment in &self.segments {
            let Some(part) = parts.next() else {
                return false;
            };
            match segment {
                Segment::Literal(value) if value != part => return false,
                Segment::Capture(_) if part.is_empty() => return false,
                _ => {}
            }
        }
        parts.next().is_none()
    }
}

/// One bounded set of decoded parameters for an explicitly exported page.
#[derive(Clone, Debug, Default)]
pub struct PageParams(pub(crate) Vec<(String, String)>);

impl PageParams {
    /// Copies unique capture names and valid decoded segment values.
    /// Route membership and completeness are checked at page registration.
    pub fn new(values: &[(&str, &str)]) -> Result<Self, AppError> {
        if values.len() > MAX_PATH_PARAMS {
            return Err(AppError::InvalidParams);
        }
        let mut params = Vec::new();
        for (key, value) in values {
            if key.len() > 64
                || !key
                    .as_bytes()
                    .first()
                    .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
                || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || params.iter().any(|(other, _)| other == key)
            {
                return Err(AppError::InvalidParams);
            }
            valid_segment(value)?;
            params.push(((*key).into(), (*value).into()));
        }
        Ok(Self(params))
    }

    pub(crate) fn pairs(&self) -> Vec<(&str, &str)> {
        self.0
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}
