use super::*;
use crate::{MAX_CLIENT_LAYOUT_DEPTH, NavigationCachePolicy, NavigationLayout};

/// A layout's public seed. Layouts have no document title or native form fallback.
pub struct ClientLayoutData {
    data: ActionData,
    no_store: bool,
}
impl ClientLayoutData {
    /// Creates a seed checked by the layout component.
    pub fn new(data: ActionData) -> Self {
        Self {
            data,
            no_store: false,
        }
    }
    /// Disables storage of the entire response containing this layout.
    pub fn no_store(mut self) -> Self {
        self.no_store = true;
        self
    }
}
#[derive(Clone)]
pub(in crate::app) struct ClientLayout {
    route: PageRoute,
    parent: Option<usize>,
    component: u32,
    state: fn(&NavigationMatch) -> Result<InitialState, PageError>,
    load: LayoutLoader,
    pub(super) cache: NavigationCachePolicy,
}
type LayoutLoader =
    Arc<dyn Fn(&PageContext<'_>) -> Result<ClientLayoutData, PageError> + Send + Sync>;
fn prefix(parent: &PageRoute, child: &PageRoute) -> bool {
    if parent.pattern() == "/" {
        return true;
    }
    parent.segments.len() <= child.segments.len()
        && parent
            .segments
            .iter()
            .zip(&child.segments)
            .all(|(a, b)| match (a, b) {
                (crate::routing::Segment::Literal(a), crate::routing::Segment::Literal(b)) => {
                    a == b
                }
                (crate::routing::Segment::Capture(a), crate::routing::Segment::Capture(b)) => {
                    a == b
                }
                _ => false,
            })
}
impl App {
    fn client_layout_index(&self, name: Option<&str>) -> Result<Option<usize>, AppError> {
        name.map(|name| {
            self.client_layouts
                .iter()
                .position(|layout| layout.route.name() == name)
                .ok_or(AppError::UnknownLayout)
        })
        .transpose()
    }
    pub(super) fn layout_chain(&self, mut leaf: Option<usize>) -> Vec<usize> {
        let mut chain = Vec::new();
        while let Some(index) = leaf {
            chain.push(index);
            leaf = self.client_layouts[index].parent;
        }
        chain.reverse();
        chain
    }
    /// Declares a persistent client layout under an already registered parent.
    /// The pattern is a segment prefix of its pages; trailing slashes are excluded.
    pub fn client_layout(
        &mut self,
        route: PageRoute,
        parent: Option<&str>,
        component: u32,
        state: fn(&NavigationMatch) -> Result<InitialState, PageError>,
        load: impl Fn(&PageContext<'_>) -> Result<ClientLayoutData, PageError> + Send + Sync + 'static,
    ) -> Result<(), AppError> {
        if self.client_factory.is_none()
            || component == 0
            || route.pattern() != "/" && route.pattern().ends_with('/')
        {
            return Err(AppError::InvalidPage);
        }
        if self
            .client_layouts
            .iter()
            .any(|old| old.route.name() == route.name())
        {
            return Err(AppError::DuplicateName);
        }
        if self.client_layouts.len() + self.layouts.len() >= MAX_APP_LAYOUTS {
            return Err(AppError::TooManyLayouts);
        }
        let parent = self.client_layout_index(parent)?;
        if self.layout_chain(parent).len() >= MAX_CLIENT_LAYOUT_DEPTH {
            return Err(AppError::LayoutDepth);
        }
        if parent.is_some_and(|index| !prefix(&self.client_layouts[index].route, &route)) {
            return Err(AppError::InvalidPage);
        }
        self.client_layouts.push(ClientLayout {
            route,
            parent,
            component,
            state,
            load: Arc::new(load),
            cache: NavigationCachePolicy::no_store(),
        });
        Ok(())
    }
    /// Selects a page's leaf client layout, or removes its layout chain.
    pub fn client_page_layout(&mut self, name: &str, layout: Option<&str>) -> Result<(), AppError> {
        let leaf = self.client_layout_index(layout)?;
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.route.name() == name)
            .ok_or(AppError::UnknownRoute)?;
        if leaf.is_some_and(|index| !prefix(&self.client_layouts[index].route, &page.route)) {
            return Err(AppError::InvalidPage);
        }
        page.client.as_mut().ok_or(AppError::InvalidPage)?.layout = leaf;
        Ok(())
    }
    /// Sets a layout's storage policy. Every ancestor and the page must opt in.
    pub fn client_layout_cache(
        &mut self,
        name: &str,
        policy: NavigationCachePolicy,
    ) -> Result<(), AppError> {
        let index = self.client_layout_index(Some(name))?.unwrap();
        self.client_layouts[index].cache = policy;
        Ok(())
    }
    fn layout_match(
        &self,
        layout: &ClientLayout,
        url: &InternalUrl,
    ) -> Result<(NavigationMatch, InternalUrl), PageError> {
        let raw = url.as_str().split('?').next().unwrap();
        let count = if layout.route.pattern() == "/" {
            0
        } else {
            layout.route.segments.len()
        };
        let target = if count == 0 {
            "/".to_owned()
        } else {
            format!(
                "/{}",
                raw[1..]
                    .split('/')
                    .take(count)
                    .collect::<Vec<_>>()
                    .join("/")
            )
        };
        let mut queried = target;
        if let Some((_, query)) = url.as_str().split_once('?') {
            queried.push('?');
            queried.push_str(query);
        }
        let mut routes = NavigationRoutes::new();
        routes
            .register(layout.route.clone(), layout.component)
            .map_err(|_| PageError::InternalServerError)?;
        let matched = routes
            .resolve(&InternalUrl::from_target(&queried).map_err(|_| PageError::BadRequest)?)
            .map_err(|_| PageError::BadRequest)?
            .ok_or(PageError::NotFound)?;
        let params = layout
            .route
            .segments
            .iter()
            .filter_map(|segment| match segment {
                crate::routing::Segment::Capture(name) => {
                    Some((name.as_str(), matched.param(name).unwrap()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let key = layout
            .route
            .url(&params)
            .map_err(|_| PageError::BadRequest)?;
        Ok((matched, key))
    }
    pub(super) fn validate_layouts(&self, packet: &NavigationPage) -> bool {
        let path = packet.url().as_str().split('?').next().unwrap();
        let Some(name) = self.owner(path) else {
            return false;
        };
        let Some(page) = self.pages.iter().find(|page| page.route.name() == name) else {
            return false;
        };
        let Some(client) = page.client else {
            return false;
        };
        let chain = self.layout_chain(client.layout);
        chain.len() == packet.layouts().len()
            && chain.iter().zip(packet.layouts()).all(|(index, seed)| {
                let layout = &self.client_layouts[*index];
                layout.route.name() == seed.name()
                    && self
                        .layout_match(layout, packet.url())
                        .is_ok_and(|(matched, key)| {
                            key == *seed.key()
                                && (layout.state)(&matched).is_ok_and(|state| {
                                    ComponentProps::new(layout.component, state)
                                        .is_ok_and(|props| props == seed.props())
                                })
                        })
            })
    }
    pub(super) fn combine_cache(
        a: NavigationCachePolicy,
        b: NavigationCachePolicy,
    ) -> NavigationCachePolicy {
        match (a.max_age_seconds(), b.max_age_seconds()) {
            (Some(a), Some(b)) => NavigationCachePolicy::public(a.min(b)).unwrap(),
            _ => NavigationCachePolicy::no_store(),
        }
    }
    pub(super) fn load_layouts(
        &self,
        page: usize,
        url: &InternalUrl,
        context: &PageContext<'_>,
    ) -> Result<(Vec<NavigationLayout>, NavigationCachePolicy), PageError> {
        let mut seeds = Vec::new();
        let mut cache = NavigationCachePolicy::public(300).unwrap();
        for index in self.layout_chain(self.pages[page].client.unwrap().layout) {
            let layout = &self.client_layouts[index];
            let (matched, key) = self.layout_match(layout, url)?;
            let props = ComponentProps::new(layout.component, (layout.state)(&matched)?)
                .map_err(|_| PageError::BadRequest)?;
            let data = (layout.load)(context)?;
            cache = Self::combine_cache(
                cache,
                if data.no_store {
                    NavigationCachePolicy::no_store()
                } else {
                    layout.cache
                },
            );
            seeds.push(
                NavigationLayout::new(layout.route.name(), key, props, data.data)
                    .map_err(|_| PageError::InternalServerError)?,
            );
        }
        Ok((seeds, cache))
    }
    pub(super) fn layout_html(name: &str, own: Html, child: Html) -> Html {
        // Names are validated ASCII identifiers.
        Html::rendered(format!(
            "<section data-ruvane-layout=\"{name}\"><div data-ruvane-layout-view=\"\">{}</div><div data-ruvane-child-outlet=\"\">{}</div></section>",
            own.as_str(),
            child.as_str()
        ))
    }
}
