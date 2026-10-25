#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod app;
mod assets;
mod forms;
mod html;
mod http;
mod metadata;
mod page_route;
mod routing;
mod server;
mod static_site;
mod url;

pub use app::{
    App, AppError, MAX_APP_LAYOUTS, MAX_APP_ROUTES, MAX_LAYOUT_DEPTH, MAX_PAGE_STYLESHEETS, Page,
    PageContext, PageError, PageMode,
};
pub use assets::{AssetError, MAX_ASSET_BYTES, MAX_ASSETS, MAX_TOTAL_ASSET_BYTES, MediaType};
pub use forms::{FormError, GetForm, MAX_FORM_NAME_BYTES};
pub use html::{
    Attribute, Document, Element, Html, ImageError, MAX_IMAGE_DIMENSION, Tag, element, fragment,
    image, link, text,
};
pub use http::{CONTENT_SECURITY_POLICY, Method, Request, Response, StatusCode};
pub use metadata::{
    CanonicalUrl, MAX_METADATA_TEXT_BYTES, MAX_SITE_ORIGIN_BYTES, Metadata, MetadataError,
    OriginError, Robots, SiteOrigin,
};
pub use page_route::{MAX_PAGE_NAME_BYTES, MAX_PAGE_SEGMENTS, PageParams, PageRoute};
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
