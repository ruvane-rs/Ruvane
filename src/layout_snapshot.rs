use crate::{MAX_NAVIGATION_BYTES, NavigationError, NavigationPage};

/// Maximum RVH1 envelope, including its pending layout packet.
pub const MAX_LAYOUT_SNAPSHOT_BYTES: usize = MAX_NAVIGATION_BYTES + 16;

/// A validated prefix of layouts for one initial document revision and URL.
#[derive(Clone, Debug)]
pub struct LayoutSnapshot {
    revision: u64,
    page: NavigationPage,
}
impl LayoutSnapshot {
    /// Wraps a nonempty, unselected RVP4 prefix. The last layout is ready to hydrate.
    pub fn new(revision: u64, page: NavigationPage) -> Result<Self, NavigationError> {
        if page.pending_layouts().is_none()
            || page.selected_segment() != 0
            || page.layouts().is_empty()
        {
            return Err(NavigationError::InvalidData);
        }
        Ok(Self { revision, page })
    }
    /// Application build revision, not authentication.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Ordered public prefix; its length identifies the newly completed layout.
    pub fn page(&self) -> &NavigationPage {
        &self.page
    }
    /// Encodes RVH1, revision u64, packet length u32 and one RVP4 packet.
    pub fn encode(&self) -> Vec<u8> {
        let page = self.page.encode();
        let mut bytes = b"RVH1".to_vec();
        bytes.extend(self.revision.to_le_bytes());
        bytes.extend((page.len() as u32).to_le_bytes());
        bytes.extend(page);
        bytes
    }
    /// Rejects incomplete, oversized, selected or nonpending layout records.
    pub fn decode(bytes: &[u8]) -> Result<Self, NavigationError> {
        if bytes.len() > MAX_LAYOUT_SNAPSHOT_BYTES {
            return Err(NavigationError::Limit);
        }
        if bytes.len() < 16 || &bytes[..4] != b"RVH1" {
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
        crate::Html::rendered(format!(
            "<div hidden data-ruvane-segment=\"\">{}<br data-ruvane-ready=\"\"></div>",
            crate::navigation_snapshot::hex(&self.encode()),
        ))
    }
}
