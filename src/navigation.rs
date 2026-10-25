//! Shared client routes and bounded RVP1 page descriptions.
use crate::{
    ActionData, ActionReply, ComponentProps, Html, InitialState, InternalUrl, PageRoute, Query,
    StatusCode,
};
use std::fmt;

/// MIME type for a typed navigation page.
pub const NAVIGATION_MEDIA_TYPE: &str = "application/vnd.ruvane.page";
/// Maximum bytes in a complete navigation packet, including layout seeds.
pub const MAX_NAVIGATION_BYTES: usize = 64 * 1024;
/// Maximum explicitly compatible routes.
pub const MAX_NAVIGATION_ROUTES: usize = 128;
/// Maximum persistent layouts in one client branch.
pub const MAX_CLIENT_LAYOUT_DEPTH: usize = 6;
/// First-party browser navigation module source.
pub const BROWSER_NAVIGATION_MODULE: &str = include_str!("../web/navigation.mjs");

/// Maximum explicitly allowed navigation freshness, in seconds.
pub const MAX_NAVIGATION_CACHE_AGE_SECONDS: u32 = 300;

/// Explicit public navigation caching. Other responses remain no-store.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NavigationCachePolicy {
    seconds: Option<u32>,
}
impl NavigationCachePolicy {
    /// Disables storage. This is the default for every page.
    pub const fn no_store() -> Self {
        Self { seconds: None }
    }
    /// Allows public, authorization-free page seeds for 1..=300 seconds.
    pub fn public(seconds: u32) -> Result<Self, NavigationError> {
        if seconds == 0 || seconds > MAX_NAVIGATION_CACHE_AGE_SECONDS {
            return Err(NavigationError::InvalidData);
        }
        Ok(Self {
            seconds: Some(seconds),
        })
    }
    /// The public freshness bound, or None for no-store.
    pub const fn max_age_seconds(self) -> Option<u32> {
        self.seconds
    }
    pub(crate) fn header(self) -> String {
        match self.seconds {
            Some(seconds) => format!("public, max-age={seconds}, must-revalidate"),
            None => "no-store".into(),
        }
    }
}

/// Invalid navigation declarations or wire data, without reflected inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationError {
    /// A route, URL, parameter or page descriptor is invalid.
    InvalidData,
    /// A byte or route-count budget was exceeded.
    Limit,
    /// A name, path or dynamic pattern conflicts with an existing route.
    Conflict,
}
impl fmt::Display for NavigationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidData => "invalid navigation data",
            Self::Limit => "navigation limit exceeded",
            Self::Conflict => "conflicting navigation route",
        })
    }
}
impl std::error::Error for NavigationError {}

/// A resolved route with decoded captures and a separate URI query.
#[derive(Clone, Debug)]
pub struct NavigationMatch {
    name: String,
    component: u32,
    params: Vec<(String, String)>,
    query: Query,
}
impl NavigationMatch {
    /// Registered route name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Registered component ID.
    pub fn component(&self) -> u32 {
        self.component
    }
    /// One capture, decoded once.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
    /// Query values, using URI decoding.
    pub fn query(&self) -> &Query {
        &self.query
    }
}

/// Routes explicitly shared by server rendering and the Wasm page validator.
#[derive(Clone, Debug, Default)]
pub struct NavigationRoutes {
    routes: Vec<(PageRoute, u32)>,
}
impl NavigationRoutes {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }
    /// Registers one route atomically. Fixed paths take precedence over captures.
    pub fn register(&mut self, route: PageRoute, component: u32) -> Result<(), NavigationError> {
        if component == 0 {
            return Err(NavigationError::InvalidData);
        }
        if self.routes.len() == MAX_NAVIGATION_ROUTES {
            return Err(NavigationError::Limit);
        }
        for (old, _) in &self.routes {
            let overlap = route.dynamic
                && old.dynamic
                && route.segments.len() == old.segments.len()
                && route
                    .segments
                    .iter()
                    .zip(&old.segments)
                    .all(|(a, b)| match (a, b) {
                        (
                            crate::routing::Segment::Literal(a),
                            crate::routing::Segment::Literal(b),
                        ) => a == b,
                        (crate::routing::Segment::Literal(value), _)
                        | (_, crate::routing::Segment::Literal(value)) => !value.is_empty(),
                        _ => true,
                    });
            if old.name() == route.name() || old.pattern() == route.pattern() || overlap {
                return Err(NavigationError::Conflict);
            }
        }
        self.routes.push((route, component));
        Ok(())
    }
    /// Resolves a validated internal target; unknown paths return None.
    pub fn resolve(&self, url: &InternalUrl) -> Result<Option<NavigationMatch>, NavigationError> {
        let (path, query) = url.as_str().split_once('?').unwrap_or((url.as_str(), ""));
        let query = Query::parse(query).map_err(|_| NavigationError::InvalidData)?;
        let selected = self
            .routes
            .iter()
            .find(|(route, _)| !route.dynamic && route.matches(path))
            .or_else(|| {
                self.routes
                    .iter()
                    .find(|(route, _)| route.dynamic && route.matches(path))
            });
        let Some((route, component)) = selected else {
            return Ok(None);
        };
        let mut params = Vec::new();
        for (segment, raw) in route.segments.iter().zip(path[1..].split('/')) {
            if let crate::routing::Segment::Capture(name) = segment {
                let value = crate::url::decode(raw)
                    .and_then(|value| {
                        crate::url::valid_segment(&value)?;
                        Ok(value)
                    })
                    .map_err(|_| NavigationError::InvalidData)?;
                params.push((name.clone(), value));
            }
        }
        Ok(Some(NavigationMatch {
            name: route.name().into(),
            component: *component,
            params,
            query,
        }))
    }
    /// Builds a registered named URL and checks that it still selects that route.
    pub fn url(&self, name: &str, params: &[(&str, &str)]) -> Result<InternalUrl, NavigationError> {
        let (route, _) = self
            .routes
            .iter()
            .find(|(route, _)| route.name() == name)
            .ok_or(NavigationError::InvalidData)?;
        let url = route
            .url(params)
            .map_err(|_| NavigationError::InvalidData)?;
        if self
            .resolve(&url)?
            .is_none_or(|matched| matched.name() != name)
        {
            return Err(NavigationError::Conflict);
        }
        Ok(url)
    }
}

/// One persistent layout's declaration, context key and public seed.
#[derive(Clone, Debug)]
pub struct NavigationLayout {
    name: String,
    key: InternalUrl,
    props: ComponentProps,
    data: ActionData,
}
impl NavigationLayout {
    /// Creates a layout descriptor. The application validates route ownership.
    pub fn new(
        name: &str,
        key: InternalUrl,
        props: ComponentProps,
        data: ActionData,
    ) -> Result<Self, NavigationError> {
        if !crate::page_route::valid_name(name) {
            return Err(NavigationError::InvalidData);
        }
        Ok(Self {
            name: name.into(),
            key,
            props,
            data,
        })
    }
    /// Registered layout name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Canonical layout context, including relevant path parameters.
    pub fn key(&self) -> &InternalUrl {
        &self.key
    }
    /// Registered component props.
    pub fn props(&self) -> ComponentProps {
        self.props
    }
    /// Initial public seed.
    pub fn data(&self) -> &ActionData {
        &self.data
    }
}

/// A page's public component seed, title and actual HTTP status.
#[derive(Clone, Debug)]
pub struct NavigationPage {
    url: InternalUrl,
    title: String,
    props: ComponentProps,
    data: ActionData,
    status: StatusCode,
    layouts: Vec<NavigationLayout>,
    selected: usize,
}
impl NavigationPage {
    /// Constructs a bounded page. Supported statuses are 200, 400 and 404.
    pub fn new(
        url: InternalUrl,
        title: &str,
        props: ComponentProps,
        data: ActionData,
        status: StatusCode,
    ) -> Result<Self, NavigationError> {
        if !matches!(
            status,
            StatusCode::Ok | StatusCode::BadRequest | StatusCode::NotFound
        ) || title.trim().is_empty()
            || title.len() > 1024
            || title.chars().any(char::is_control)
        {
            return Err(NavigationError::InvalidData);
        }
        let page = Self {
            url,
            title: title.into(),
            props,
            data,
            status,
            layouts: Vec::new(),
            selected: 0,
        };
        if page.encode_unchecked().len() > MAX_NAVIGATION_BYTES {
            return Err(NavigationError::Limit);
        }
        Ok(page)
    }
    /// Attaches root-first layouts within the complete packet byte budget.
    pub fn with_layouts(mut self, layouts: Vec<NavigationLayout>) -> Result<Self, NavigationError> {
        if layouts.len() > MAX_CLIENT_LAYOUT_DEPTH {
            return Err(NavigationError::Limit);
        }
        if self.status != StatusCode::Ok && !layouts.is_empty() {
            return Err(NavigationError::InvalidData);
        }
        for (index, layout) in layouts.iter().enumerate() {
            if layouts[..index].iter().any(|old| old.name == layout.name) {
                return Err(NavigationError::InvalidData);
            }
        }
        self.layouts = layouts;
        self.selected = 0;
        if self.encode_unchecked().len() > MAX_NAVIGATION_BYTES {
            return Err(NavigationError::Limit);
        }
        Ok(self)
    }
    /// Root-first persistent layout descriptors.
    pub fn layouts(&self) -> &[NavigationLayout] {
        &self.layouts
    }
    /// Selects a layout for mounting. Zero selects the leaf page.
    pub fn select_segment(mut self, selected: usize) -> Result<Self, NavigationError> {
        if selected > self.layouts.len() {
            return Err(NavigationError::InvalidData);
        }
        self.selected = selected;
        Ok(self)
    }
    /// Selected segment: zero is the page, followed by root-first layouts.
    pub fn selected_segment(&self) -> usize {
        self.selected
    }
    /// Props and seed for the selected segment.
    pub fn segment(&self) -> (ComponentProps, &ActionData) {
        if self.selected == 0 {
            (self.props, &self.data)
        } else {
            let layout = &self.layouts[self.selected - 1];
            (layout.props, &layout.data)
        }
    }
    /// Exact validated target, including its query spelling.
    pub fn url(&self) -> &InternalUrl {
        &self.url
    }
    /// Plain document title.
    pub fn title(&self) -> &str {
        &self.title
    }
    /// Registered component props.
    pub fn props(&self) -> ComponentProps {
        self.props
    }
    /// Public typed seed; application schemas must be validated before rendering.
    pub fn data(&self) -> &ActionData {
        &self.data
    }
    /// HTTP status, also carried in the packet.
    pub fn status(&self) -> StatusCode {
        self.status
    }
    fn encode_unchecked(&self) -> Vec<u8> {
        let mut bytes = if self.layouts.is_empty() {
            b"RVP1".to_vec()
        } else {
            let mut bytes = b"RVP2".to_vec();
            bytes.push(self.selected as u8);
            bytes
        };
        bytes.extend_from_slice(&self.status.as_u16().to_le_bytes());
        for text in [self.url.as_str(), self.title.as_str()] {
            bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
        }
        bytes.extend_from_slice(&self.props.component().to_le_bytes());
        let (kind, value) = self.props.state().to_wire();
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
        let seed = ActionReply::success(self.data.clone()).encode();
        bytes.extend_from_slice(&(seed.len() as u32).to_le_bytes());
        bytes.extend(seed);
        if !self.layouts.is_empty() {
            bytes.push(self.layouts.len() as u8);
            for layout in &self.layouts {
                for text in [layout.name.as_str(), layout.key.as_str()] {
                    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
                    bytes.extend_from_slice(text.as_bytes());
                }
                bytes.extend_from_slice(&layout.props.component().to_le_bytes());
                let (kind, value) = layout.props.state().to_wire();
                bytes.extend_from_slice(&kind.to_le_bytes());
                bytes.extend_from_slice(&value.to_le_bytes());
                let seed = ActionReply::success(layout.data.clone()).encode();
                bytes.extend_from_slice(&(seed.len() as u32).to_le_bytes());
                bytes.extend(seed);
            }
        }
        bytes
    }
    /// Encodes RVP1 for a leaf page, or RVP2 for a layout branch.
    pub fn encode(&self) -> Vec<u8> {
        self.encode_unchecked()
    }
    /// Validates framing, status, URL, title, props and seed before exposing a page.
    pub fn decode(bytes: &[u8]) -> Result<Self, NavigationError> {
        if bytes.len() > MAX_NAVIGATION_BYTES {
            return Err(NavigationError::Limit);
        }
        struct Reader<'a> {
            bytes: &'a [u8],
            pos: usize,
        }
        impl<'a> Reader<'a> {
            fn take(&mut self, length: usize) -> Result<&'a [u8], NavigationError> {
                let end = self
                    .pos
                    .checked_add(length)
                    .ok_or(NavigationError::InvalidData)?;
                let value = self
                    .bytes
                    .get(self.pos..end)
                    .ok_or(NavigationError::InvalidData)?;
                self.pos = end;
                Ok(value)
            }
            fn number(&mut self) -> Result<u32, NavigationError> {
                Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
            }
            fn text(&mut self, limit: usize) -> Result<&'a str, NavigationError> {
                let length = self.number()? as usize;
                if length > limit {
                    return Err(NavigationError::Limit);
                }
                std::str::from_utf8(self.take(length)?).map_err(|_| NavigationError::InvalidData)
            }
        }
        let mut reader = Reader { bytes, pos: 0 };
        let version = reader.take(4)?;
        if version != b"RVP1" && version != b"RVP2" {
            return Err(NavigationError::InvalidData);
        }
        let selected = if version == b"RVP2" {
            reader.take(1)?[0] as usize
        } else {
            0
        };
        let status = match u16::from_le_bytes(reader.take(2)?.try_into().unwrap()) {
            200 => StatusCode::Ok,
            400 => StatusCode::BadRequest,
            404 => StatusCode::NotFound,
            _ => return Err(NavigationError::InvalidData),
        };
        let url = InternalUrl::from_target(reader.text(crate::MAX_INTERNAL_URL_BYTES)?)
            .map_err(|_| NavigationError::InvalidData)?;
        let title = reader.text(1024)?;
        let component = reader.number()?;
        let state = InitialState::from_wire(reader.number()?, reader.number()?)
            .map_err(|_| NavigationError::InvalidData)?;
        let props =
            ComponentProps::new(component, state).map_err(|_| NavigationError::InvalidData)?;
        let length = reader.number()? as usize;
        let seed =
            ActionReply::decode(reader.take(length)?).map_err(|_| NavigationError::InvalidData)?;
        if seed.is_validation() {
            return Err(NavigationError::InvalidData);
        }
        let mut layouts = Vec::new();
        if version == b"RVP2" {
            let count = reader.take(1)?[0] as usize;
            if count == 0 || count > MAX_CLIENT_LAYOUT_DEPTH {
                return Err(NavigationError::Limit);
            }
            for _ in 0..count {
                let name = reader.text(crate::MAX_PAGE_NAME_BYTES)?;
                let key = InternalUrl::from_target(reader.text(crate::MAX_INTERNAL_URL_BYTES)?)
                    .map_err(|_| NavigationError::InvalidData)?;
                let component = reader.number()?;
                let state = InitialState::from_wire(reader.number()?, reader.number()?)
                    .map_err(|_| NavigationError::InvalidData)?;
                let props = ComponentProps::new(component, state)
                    .map_err(|_| NavigationError::InvalidData)?;
                let length = reader.number()? as usize;
                let data = ActionReply::decode(reader.take(length)?)
                    .map_err(|_| NavigationError::InvalidData)?;
                if data.is_validation() {
                    return Err(NavigationError::InvalidData);
                }
                layouts.push(NavigationLayout::new(
                    name,
                    key,
                    props,
                    data.data().clone(),
                )?);
            }
        }
        if reader.pos != bytes.len() {
            return Err(NavigationError::InvalidData);
        }
        Self::new(url, title, props, seed.data().clone(), status)?
            .with_layouts(layouts)?
            .select_segment(selected)
    }
}

/// A normal HTML link explicitly eligible for client navigation.
pub fn navigation_link(url: &InternalUrl, label: &str) -> Html {
    let mut html = String::from("<a data-ruvane-nav=\"\" href=\"");
    crate::html::escape_into(&mut html, url.as_str());
    html.push_str("\">");
    crate::html::escape_into(&mut html, label);
    html.push_str("</a>");
    Html::rendered(html)
}

/// Marks a public native link for speculative GET loading. The response policy
/// still controls storage; callers must only mark safe, read-only destinations.
pub fn prefetch_link(url: &InternalUrl, label: &str) -> Html {
    let mut html = String::from("<a data-ruvane-nav=\"\" data-ruvane-prefetch=\"\" href=\"");
    crate::html::escape_into(&mut html, url.as_str());
    html.push_str("\">");
    crate::html::escape_into(&mut html, label);
    html.push_str("</a>");
    Html::rendered(html)
}
