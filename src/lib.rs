#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod app;
mod assets;
mod browser;
mod content;
mod forms;
mod html;
mod http;
mod metadata;
mod page_route;
mod reactive;
mod routing;
mod server;
mod static_site;
mod url;

pub use app::{
    App, AppError, MAX_APP_LAYOUTS, MAX_APP_ROUTES, MAX_LAYOUT_DEPTH, MAX_PAGE_STYLESHEETS, Page,
    PageContext, PageError, PageMode,
};
pub use assets::{AssetError, MAX_ASSET_BYTES, MAX_ASSETS, MAX_TOTAL_ASSET_BYTES, MediaType};
pub use browser::{
    BROWSER_ABI_VERSION, BROWSER_BRIDGE_MODULE, BrowserCommandError, BrowserCommands, BrowserTag,
    MAX_BROWSER_COMMAND_BYTES, MAX_BROWSER_COMMANDS, MAX_BROWSER_TEXT_BYTES,
};
pub use content::{
    ContentBundle, ContentCollection, ContentEntry, ContentError, ContentErrorKind, ContentSlug,
    ContentSource, MAX_CONTENT_FILE_BYTES, MAX_CONTENT_FILES, MAX_CONTENT_HTML_BYTES,
    MAX_CONTENT_LINE_BYTES, MAX_CONTENT_LINES, MAX_CONTENT_MODULE_BYTES, MAX_CONTENT_TOTAL_BYTES,
    MAX_CONTENT_TOTAL_HTML_BYTES, MAX_MARKDOWN_DEPTH, MAX_MARKDOWN_TOKENS, MAX_MARKDOWN_WORK,
    markdown,
};
pub use forms::{FormError, GetForm, MAX_FORM_NAME_BYTES};
pub use html::{
    Attribute, Document, Element, Html, ImageError, MAX_IMAGE_DIMENSION, Tag, element, fragment,
    image, link, module_script, text,
};
pub use http::{
    CONTENT_SECURITY_POLICY, INTERACTIVE_CONTENT_SECURITY_POLICY, Method, Request, Response,
    StatusCode,
};
pub use metadata::{
    CanonicalUrl, MAX_METADATA_TEXT_BYTES, MAX_SITE_ORIGIN_BYTES, Metadata, MetadataError,
    OriginError, Robots, SiteOrigin,
};
pub use page_route::{MAX_PAGE_NAME_BYTES, MAX_PAGE_SEGMENTS, PageParams, PageRoute};
pub use reactive::{
    Effect, Memo, ReactiveError, ReactiveLimits, ReactiveRuntime, ReactiveStats, Scope, Signal,
    Source,
};
pub use routing::{RouteError, Router};
pub use server::{Server, ServerConfig, ShutdownHandle};
pub use static_site::{
    ExportError, ExportReport, MAX_STATIC_ENTRIES, MAX_STATIC_FILE_BYTES, MAX_STATIC_PATH_BYTES,
    MAX_STATIC_SEGMENTS, MAX_STATIC_TOTAL_BYTES, STATIC_COMPLETE_PATH, STATIC_HEADERS_PATH,
    STATIC_MANIFEST_PATH, StaticPath, StaticSite,
};
pub use url::{
    InternalUrl, MAX_COMPONENT_BYTES, MAX_INTERNAL_URL_BYTES, MAX_PATH_PARAMS, MAX_QUERY_PAIRS,
    ParamError, Query, QueryMode, UrlError,
};
