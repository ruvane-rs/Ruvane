use std::fmt;

/// Maximum declared bytes in one embedded asset (1 MiB).
pub const MAX_ASSET_BYTES: usize = 1024 * 1024;
/// Maximum number of embedded assets registered in one router.
pub const MAX_ASSETS: usize = 64;
/// Maximum sum of declared asset sizes in one router (8 MiB).
///
/// Registering the same slice at different paths counts its length each time.
pub const MAX_TOTAL_ASSET_BYTES: usize = 8 * 1024 * 1024;

/// A fixed MIME type for an opaque byte body or trusted embedded asset.
///
/// Types are explicitly declared, never inferred from a path or file contents.
/// Ruvane does not decode images, validate CSS, or sanitize asset bytes. CSS is
/// trusted application code. HTML requires the renderer and `Response::html`;
/// there is no raw HTML, SVG, or JavaScript variant in this byte API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MediaType {
    /// UTF-8 plain text.
    PlainText,
    /// UTF-8 CSS, supplied by the application as trusted source.
    Css,
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
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::WebP => "image/webp",
            Self::OctetStream => "application/octet-stream",
        }
    }
}

/// An embedded asset registration failure, without reflected path or contents.
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
