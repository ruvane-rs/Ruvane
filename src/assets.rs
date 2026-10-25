use std::fmt;

/// Maximum declared bytes in one embedded asset (1 MiB).
pub const MAX_ASSET_BYTES: usize = 1024 * 1024;
/// Maximum number of embedded assets registered in one router.
pub const MAX_ASSETS: usize = 64;
/// Maximum sum of declared asset sizes in one router (8 MiB).
///
/// Registering the same slice at different paths counts its length each time.
pub const MAX_TOTAL_ASSET_BYTES: usize = 8 * 1024 * 1024;

/// MIME types for byte responses and embedded assets.
///
/// Types are declared by the application; bytes are not validated or sanitized.
/// CSS, JavaScript and Wasm must come from trusted sources. HTML uses the renderer
/// and `Response::html`; this API has no raw HTML or SVG type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MediaType {
    /// UTF-8 plain text.
    PlainText,
    /// UTF-8 CSS, supplied by the application as trusted source.
    Css,
    /// Trusted JavaScript module source; execution requires an interactive page.
    JavaScript,
    /// A trusted WebAssembly module.
    WebAssembly,
    /// An already encoded PNG image.
    Png,
    /// An already encoded JPEG image.
    Jpeg,
    /// An already encoded WebP image.
    WebP,
    /// Uninterpreted bytes.
    OctetStream,
}

impl MediaType {
    /// Returns the fixed Content-Type value, with a charset for text types.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PlainText => "text/plain; charset=utf-8",
            Self::Css => "text/css; charset=utf-8",
            Self::JavaScript => "text/javascript; charset=utf-8",
            Self::WebAssembly => "application/wasm",
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::WebP => "image/webp",
            Self::OctetStream => "application/octet-stream",
        }
    }
}

/// Asset registration errors. Messages contain no paths or file contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetError {
    /// The URL is not a valid, fixed internal path.
    InvalidPath,
    /// A GET route or asset already occupies the exact path.
    DuplicatePath,
    /// One asset exceeds `MAX_ASSET_BYTES`.
    TooLarge,
    /// Registration would exceed `MAX_ASSETS`.
    TooManyAssets,
    /// Registration would exceed `MAX_TOTAL_ASSET_BYTES`.
    TotalTooLarge,
}

impl fmt::Display for AssetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPath => "invalid asset path",
            Self::DuplicatePath => "duplicate asset path",
            Self::TooLarge => "asset byte limit exceeded",
            Self::TooManyAssets => "asset count limit exceeded",
            Self::TotalTooLarge => "total asset byte limit exceeded",
        })
    }
}

impl std::error::Error for AssetError {}
