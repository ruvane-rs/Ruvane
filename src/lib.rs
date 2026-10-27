#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod actions;
mod app;
mod assets;
mod browser;
mod content;
mod cookies;
mod csrf;
mod development;
mod file_routes;
mod forms;
mod guards;
mod html;
mod http;
mod json;
mod layout_snapshot;
mod metadata;
mod navigation;
mod navigation_snapshot;
mod page_route;
mod prerender;
#[cfg(not(target_family = "wasm"))]
mod rate_limit;
mod reactive;
mod routing;
mod server;
mod server_cache;
#[cfg(not(target_family = "wasm"))]
mod sessions;
mod static_site;
mod streaming;
mod url;
mod view;

pub use actions::{
    ACTION_MEDIA_TYPE, ActionData, ActionError, ActionFailure, ActionOutcome, ActionReply,
    ActionRequest, ActionValue, MAX_ACTION_FIELDS, MAX_ACTION_REPLY_BYTES,
    MAX_ACTION_REQUEST_BYTES,
};
pub use app::{
    App, AppError, ClientBoundary, ClientDocument, ClientDocumentHead, ClientLayoutData,
    ClientPageData, ClientRuntime, ComponentFactory, MAX_APP_LAYOUTS, MAX_APP_ROUTES,
    MAX_LAYOUT_DEPTH, MAX_PAGE_STYLESHEETS, Page, PageContext, PageError, PageMode,
    application_revision,
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
pub use cookies::{
    Cookie, CookieError, Cookies, MAX_COOKIE_AGE_SECONDS, MAX_COOKIE_HEADER_BYTES,
    MAX_COOKIE_NAME_BYTES, MAX_COOKIE_PATH_BYTES, MAX_COOKIE_VALUE_BYTES, MAX_REQUEST_COOKIES,
    MAX_RESPONSE_COOKIE_BYTES, MAX_RESPONSE_COOKIES, MAX_SET_COOKIE_BYTES, SameSite,
};
pub use csrf::{
    CSRF_FORM_FIELD, CSRF_HEADER, CSRF_MEDIA_TYPE, CSRF_TOKEN_BYTES, CSRF_TOKEN_LENGTH, CsrfError,
    CsrfToken,
};
pub use development::{Development, MAX_DEVELOPMENT_STATUS_BYTES};
pub use file_routes::{
    FileEndpoint, FileError, FileLayout, FileLoading, FileNotFound, FilePage, FileRouteBoundary,
    FileRouteError, FileRouteLayout, FileRouteNotFound, FileRouteRegistry, FileRoutes,
    MAX_FILE_ROUTE_FILES, MAX_FILE_ROUTE_MODULE_BYTES, MAX_FILE_ROUTE_SOURCE_BYTES,
    MAX_FILE_ROUTE_TOTAL_BYTES,
};
pub use forms::{FormData, FormError, GetForm, MAX_FORM_NAME_BYTES, MAX_POST_BODY_BYTES, PostForm};
#[doc(hidden)]
pub use guards::guard_component;
pub use guards::{
    GUARD_COMPONENT_ID, GuardContext, GuardDecision, MAX_APP_GUARDS, MAX_ROUTE_GUARDS, RouteGuard,
};
pub use html::{
    Attribute, Document, Element, Html, ImageError, MAX_IMAGE_DIMENSION, Tag, element, fragment,
    image, link, module_script, text,
};
pub use http::{
    CONTENT_SECURITY_POLICY, INTERACTIVE_CONTENT_SECURITY_POLICY, MAX_RETRY_AFTER_SECONDS, Method,
    Request, Response, RetryAfter, StatusCode,
};
pub use json::{
    JSON_MEDIA_TYPE, JsonError, JsonNumber, JsonValue, MAX_JSON_BYTES, MAX_JSON_DEPTH,
    MAX_JSON_ITEMS, MAX_JSON_NUMBER_BYTES, MAX_JSON_STRING_BYTES, MAX_JSON_VALUES,
};
pub use layout_snapshot::{LayoutSnapshot, MAX_LAYOUT_SNAPSHOT_BYTES};
pub use metadata::{
    CanonicalUrl, MAX_METADATA_TEXT_BYTES, MAX_SITE_ORIGIN_BYTES, Metadata, MetadataError,
    OriginError, Robots, SiteOrigin,
};
pub use navigation::{
    BROWSER_NAVIGATION_MODULE, MAX_CLIENT_LAYOUT_DEPTH, MAX_NAVIGATION_BYTES,
    MAX_NAVIGATION_CACHE_AGE_SECONDS, MAX_NAVIGATION_METADATA_BYTES, MAX_NAVIGATION_ROUTES,
    MAX_NAVIGATION_STREAM_BYTES, NAVIGATION_MEDIA_TYPE, NAVIGATION_STREAM_MEDIA_TYPE,
    NavigationCachePolicy, NavigationError, NavigationLayout, NavigationMatch, NavigationPage,
    NavigationRoutes, navigation_link, prefetch_link,
};
pub use navigation_snapshot::{MAX_NAVIGATION_SNAPSHOT_BYTES, NavigationSnapshot};
pub use page_route::{MAX_PAGE_NAME_BYTES, MAX_PAGE_SEGMENTS, PageParams, PageRoute};
pub use prerender::{
    CLIENT_PRERENDER_MARKER, ClientPrerender, MAX_CLIENT_PRERENDER_BYTES,
    MAX_CLIENT_PRERENDER_PAGES, PrerenderError, PrerenderRevalidationPolicy,
};
#[cfg(not(target_family = "wasm"))]
pub use rate_limit::{
    MAX_RATE_LIMIT_ENTRIES, MAX_RATE_LIMIT_PERIOD_SECONDS, MAX_RATE_LIMIT_REQUESTS,
    MAX_RATE_LIMIT_SCOPE_BYTES, RateLimitDecision, RateLimitError, RateLimitLimits,
    RateLimitPolicy, RateLimitStats, RateLimiter,
};
pub use reactive::{
    Effect, Memo, ReactiveError, ReactiveLimits, ReactiveRuntime, ReactiveStats, Scope, Signal,
    Source,
};
pub use routing::{RouteError, Router};
pub use server::{Server, ServerConfig, ShutdownHandle};
pub use server_cache::{
    MAX_SERVER_CACHE_BYTES, MAX_SERVER_CACHE_DISK_BYTES, MAX_SERVER_CACHE_ENTRIES,
    MAX_SERVER_CACHE_LOADS, MAX_SERVER_CACHE_QUEUE, MAX_SERVER_CACHE_STALE_SECONDS,
    MAX_SERVER_CACHE_TAGS, MAX_SERVER_CACHE_WAIT_MS, MAX_SERVER_CACHE_WAITERS,
    MAX_SERVER_CACHE_WORKERS, ServerCache, ServerCacheBackgroundLimits, ServerCacheDiskLimits,
    ServerCacheError, ServerCacheLimits, ServerCachePersistence, ServerCachePolicy,
    ServerCacheStats,
};
#[cfg(not(target_family = "wasm"))]
pub use sessions::{
    MAX_SESSION_DATA_BYTES, MAX_SESSION_ENTRIES, MAX_SESSION_FIELDS, MAX_SESSION_LIFETIME_SECONDS,
    MAX_SESSION_STORE_BYTES, MAX_SESSION_VALUE_BYTES, SESSION_ID_BYTES, Session, SessionCookie,
    SessionData, SessionError, SessionLimits, SessionStats, SessionStore,
};
pub use static_site::{
    ExportError, ExportReport, MAX_STATIC_ENTRIES, MAX_STATIC_FILE_BYTES, MAX_STATIC_PATH_BYTES,
    MAX_STATIC_SEGMENTS, MAX_STATIC_TOTAL_BYTES, STATIC_COMPLETE_PATH, STATIC_HEADERS_PATH,
    STATIC_MANIFEST_PATH, StaticPath, StaticSite,
};
pub use streaming::{
    HtmlProducer, HtmlStream, MAX_STREAM_BODY_BYTES, MAX_STREAM_CHUNK_BYTES, MAX_STREAM_CHUNKS,
    StreamContext, StreamError, StreamLimits,
};
pub use url::{
    InternalUrl, MAX_COMPONENT_BYTES, MAX_INTERNAL_URL_BYTES, MAX_PATH_PARAMS, MAX_QUERY_PAIRS,
    ParamError, Query, QueryMode, UrlError,
};
pub use view::{
    Component, ComponentError, ComponentProps, ComponentStats, EventValue, InitialState,
    MAX_VIEW_DEPTH, MAX_VIEW_EVENTS, MAX_VIEW_NODES, View, ViewError, ViewSnapshot,
};
