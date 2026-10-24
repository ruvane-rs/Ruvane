#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod html;

pub use html::{Attribute, Document, Element, Html, Tag, element, fragment, text};
