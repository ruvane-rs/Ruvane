#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod assets;
mod html;
mod http;
mod routing;
mod server;
mod url;

pub use assets::{AssetError, MAX_ASSET_BYTES, MAX_ASSETS, MAX_TOTAL_ASSET_BYTES, MediaType};
pub use html::{
    Attribute, Document, Element, Html, ImageError, MAX_IMAGE_DIMENSION, Tag, element, fragment,
    image, link, text,
};
pub use http::{Method, Request, Response, StatusCode};
pub use routing::{RouteError, Router};
pub use server::{Server, ServerConfig, ShutdownHandle};
pub use url::{
    InternalUrl, MAX_COMPONENT_BYTES, MAX_INTERNAL_URL_BYTES, MAX_PATH_PARAMS, MAX_QUERY_PAIRS,
    ParamError, Query, UrlError,
};
