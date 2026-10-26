use super::*;
use crate::NavigationError;

impl Metadata {
    pub(crate) fn encode_wire(&self) -> Vec<u8> {
        let mut bytes = vec![
            u8::from(self.description.is_some())
                | (u8::from(self.robots.is_some()) << 1)
                | (u8::from(self.canonical.is_some()) << 2)
                | (u8::from(self.favicon_png.is_some()) << 3),
        ];
        fn text(bytes: &mut Vec<u8>, value: &str) {
            bytes.extend((value.len() as u32).to_le_bytes());
            bytes.extend(value.as_bytes());
        }
        if let Some(value) = &self.description {
            text(&mut bytes, value);
        }
        if let Some(value) = self.robots {
            bytes.push(match value {
                Robots::IndexFollow => 1,
                Robots::IndexNoFollow => 2,
                Robots::NoIndexFollow => 3,
                Robots::NoIndexNoFollow => 4,
            });
        }
        if let Some(value) = &self.canonical {
            text(&mut bytes, value.as_str());
        }
        if let Some(value) = &self.favicon_png {
            text(&mut bytes, value.as_str());
        }
        bytes
    }
    pub(crate) fn decode_wire(bytes: &[u8]) -> Result<Self, NavigationError> {
        struct Reader<'a> {
            bytes: &'a [u8],
        }
        impl<'a> Reader<'a> {
            fn take(&mut self, count: usize) -> Result<&'a [u8], NavigationError> {
                let value = self
                    .bytes
                    .get(..count)
                    .ok_or(NavigationError::InvalidData)?;
                self.bytes = &self.bytes[count..];
                Ok(value)
            }
            fn text(&mut self, limit: usize) -> Result<&'a str, NavigationError> {
                let count = u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as usize;
                if count > limit {
                    return Err(NavigationError::Limit);
                }
                std::str::from_utf8(self.take(count)?).map_err(|_| NavigationError::InvalidData)
            }
        }
        let mut reader = Reader { bytes };
        let flags = reader.take(1)?[0];
        if flags > 15 {
            return Err(NavigationError::InvalidData);
        }
        let mut metadata = Self::new();
        if flags & 1 != 0 {
            metadata = metadata
                .description(reader.text(MAX_METADATA_TEXT_BYTES)?)
                .map_err(|_| NavigationError::InvalidData)?;
        }
        if flags & 2 != 0 {
            metadata.robots = Some(match reader.take(1)?[0] {
                1 => Robots::IndexFollow,
                2 => Robots::IndexNoFollow,
                3 => Robots::NoIndexFollow,
                4 => Robots::NoIndexNoFollow,
                _ => return Err(NavigationError::InvalidData),
            });
        }
        if flags & 4 != 0 {
            let value = reader.text(MAX_SITE_ORIGIN_BYTES + MAX_INTERNAL_URL_BYTES)?;
            let authority = value.find("://").ok_or(NavigationError::InvalidData)? + 3;
            let split = authority
                + value[authority..]
                    .find('/')
                    .ok_or(NavigationError::InvalidData)?;
            let origin =
                SiteOrigin::parse(&value[..split]).map_err(|_| NavigationError::InvalidData)?;
            let path = InternalUrl::from_target(&value[split..])
                .map_err(|_| NavigationError::InvalidData)?;
            let canonical = origin.canonical(&path);
            if canonical.as_str() != value {
                return Err(NavigationError::InvalidData);
            }
            metadata.canonical = Some(canonical);
        }
        if flags & 8 != 0 {
            metadata.favicon_png = Some(
                InternalUrl::from_target(reader.text(MAX_INTERNAL_URL_BYTES)?)
                    .map_err(|_| NavigationError::InvalidData)?,
            );
        }
        if !reader.bytes.is_empty() {
            return Err(NavigationError::InvalidData);
        }
        Ok(metadata)
    }
    pub(crate) fn accepts_origin(&self, origin: Option<&SiteOrigin>) -> bool {
        self.canonical.as_ref().is_none_or(|canonical| {
            origin.is_some_and(|origin| {
                canonical
                    .as_str()
                    .strip_prefix(origin.as_str())
                    .is_some_and(|tail| tail.starts_with('/'))
            })
        })
    }
}
