#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod html;
mod http;
mod routing;
mod server;

pub use html::{Attribute, Document, Element, Html, Tag, element, fragment, text};
pub use http::{Method, Request, Response, StatusCode};
pub use routing::{RouteError, Router};
pub use server::{Server, ServerConfig, ShutdownHandle};
