//! RVN1 browser command encoding.
use std::fmt;

/// Browser command and export ABI version.
pub const BROWSER_ABI_VERSION: u32 = 1;
/// Maximum bytes in one command batch, including its eight-byte header.
pub const MAX_BROWSER_COMMAND_BYTES: usize = 64 * 1024;
/// Maximum commands in one batch.
pub const MAX_BROWSER_COMMANDS: usize = 128;
/// Maximum UTF-8 bytes in one text update.
pub const MAX_BROWSER_TEXT_BYTES: usize = 1024;
/// JavaScript source for the DOM bridge.
pub const BROWSER_BRIDGE_MODULE: &str = include_str!("../web/bridge.mjs");

/// Elements supported by the DOM bridge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum BrowserTag {
    /// A component section.
    Section = 1,
    /// A second-level heading.
    Heading = 2,
    /// A paragraph container.
    Paragraph = 3,
    /// Inline text.
    Span = 4,
    /// A computed value, exposed as a status to assistive tools.
    Output = 5,
    /// A native button, always type=button.
    Button = 6,
}

/// Command encoding errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserCommandError {
    /// A required view, node or event identifier is zero.
    InvalidIdentifier,
    /// The batch, command count or text byte limit would be exceeded.
    Limit,
}

impl fmt::Display for BrowserCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidIdentifier => "invalid browser command identifier",
            Self::Limit => "browser command limit exceeded",
        })
    }
}
impl std::error::Error for BrowserCommandError {}

/// A command batch. Failed appends leave it unchanged.
///
/// Supports the operations below; text stays plain text. The bridge checks DOM ownership.
#[derive(Clone, Debug)]
pub struct BrowserCommands {
    bytes: Vec<u8>,
    count: usize,
}

impl Default for BrowserCommands {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserCommands {
    /// Creates an empty batch with an RVN1 header.
    pub fn new() -> Self {
        Self {
            bytes: b"RVN1\0\0\0\0".to_vec(),
            count: 0,
        }
    }

    fn push(&mut self, opcode: u8, ids: &[u32], data: &[u8]) -> Result<(), BrowserCommandError> {
        let size = 1 + ids.len() * 4 + data.len();
        if self.count == MAX_BROWSER_COMMANDS || self.bytes.len() + size > MAX_BROWSER_COMMAND_BYTES
        {
            return Err(BrowserCommandError::Limit);
        }
        self.bytes.push(opcode);
        for id in ids {
            self.bytes.extend_from_slice(&id.to_le_bytes());
        }
        self.bytes.extend_from_slice(data);
        self.count += 1;
        self.bytes[4..8].copy_from_slice(&(self.count as u32).to_le_bytes());
        Ok(())
    }

    fn ids(ids: &[u32]) -> Result<(), BrowserCommandError> {
        if ids.contains(&0) {
            Err(BrowserCommandError::InvalidIdentifier)
        } else {
            Ok(())
        }
    }

    /// Creates an element. Parent zero means the view's host root.
    pub fn element(
        &mut self,
        view: u32,
        node: u32,
        parent: u32,
        tag: BrowserTag,
    ) -> Result<(), BrowserCommandError> {
        Self::ids(&[view, node])?;
        self.push(1, &[view, node, parent], &[tag as u8])
    }

    /// Sets leaf text, up to `MAX_BROWSER_TEXT_BYTES` UTF-8 bytes.
    pub fn text(&mut self, view: u32, node: u32, text: &str) -> Result<(), BrowserCommandError> {
        Self::ids(&[view, node])?;
        if text.len() > MAX_BROWSER_TEXT_BYTES {
            return Err(BrowserCommandError::Limit);
        }
        self.push(2, &[view, node, text.len() as u32], text.as_bytes())
    }

    /// Connects a button click to a Rust event ID.
    pub fn click(&mut self, view: u32, node: u32, event: u32) -> Result<(), BrowserCommandError> {
        Self::ids(&[view, node, event])?;
        self.push(3, &[view, node, event], &[])
    }

    /// Enables or disables a button.
    pub fn disabled(
        &mut self,
        view: u32,
        node: u32,
        disabled: bool,
    ) -> Result<(), BrowserCommandError> {
        Self::ids(&[view, node])?;
        self.push(4, &[view, node], &[u8::from(disabled)])
    }

    /// Removes a view's nodes and listeners.
    pub fn remove(&mut self, view: u32) -> Result<(), BrowserCommandError> {
        Self::ids(&[view])?;
        self.push(5, &[view], &[])
    }

    /// Borrows the batch. The slice is valid only until the next mutation.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the command count, excluding the header.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Whether the batch has no commands. The header is always present.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Clears the batch, keeping its allocation.
    pub fn clear(&mut self) {
        self.bytes.truncate(8);
        self.bytes[4..8].fill(0);
        self.count = 0;
    }
}
