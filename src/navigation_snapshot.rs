use crate::{MAX_NAVIGATION_BYTES, NavigationError, NavigationPage};

/// Maximum decoded RVI1 snapshot, including its envelope.
pub const MAX_NAVIGATION_SNAPSHOT_BYTES: usize = MAX_NAVIGATION_BYTES + 16;

/// A complete server page and the exact application module revision.
#[derive(Clone, Debug)]
pub struct NavigationSnapshot {
    revision: u64,
    page: NavigationPage,
}
impl NavigationSnapshot {
    /// Wraps a complete, unselected page. Pending prefixes cannot initialize a document.
    pub fn new(revision: u64, page: NavigationPage) -> Result<Self, NavigationError> {
        if page.pending_layouts().is_some() || page.selected_segment() != 0 {
            return Err(NavigationError::InvalidData);
        }
        Ok(Self { revision, page })
    }
    /// Application build revision, not authentication.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Complete typed server page, with its logical status and public seeds.
    pub fn page(&self) -> &NavigationPage {
        &self.page
    }
    /// Encodes RVI1, revision u64, packet length u32 and the complete navigation packet.
    pub fn encode(&self) -> Vec<u8> {
        let page = self.page.encode();
        let mut bytes = b"RVI1".to_vec();
        bytes.extend(self.revision.to_le_bytes());
        bytes.extend((page.len() as u32).to_le_bytes());
        bytes.extend(page);
        bytes
    }
    /// Checks the envelope, exact length and complete packet before exposing data.
    pub fn decode(bytes: &[u8]) -> Result<Self, NavigationError> {
        if bytes.len() > MAX_NAVIGATION_SNAPSHOT_BYTES {
            return Err(NavigationError::Limit);
        }
        if bytes.len() < 16 || &bytes[..4] != b"RVI1" {
            return Err(NavigationError::InvalidData);
        }
        let revision = u64::from_le_bytes(bytes[4..12].try_into().unwrap());
        let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        if length != bytes.len() - 16 {
            return Err(NavigationError::InvalidData);
        }
        Self::new(revision, NavigationPage::decode(&bytes[16..])?)
    }
    pub(crate) fn html(&self) -> crate::Html {
        let mut markup = String::from("<div hidden id=\"ruvane-initial\">");
        markup.push_str(&hex(&self.encode()));
        markup.push_str("</div>");
        crate::Html::rendered(markup)
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 15) as usize] as char);
    }
    text
}
