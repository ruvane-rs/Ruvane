use ruvane::{
    App, Attribute, ContentCollection, ContentEntry, ContentSource, GetForm, Html, InternalUrl,
    MediaType, Metadata, Page, PageContext, PageError, PageMode, PageParams, PageRoute, ParamError,
    QueryMode, Response, Robots, Router, SiteOrigin, StaticSite, Tag, element, fragment, image,
    link,
};

const PAGE_SIZE: usize = 2;

static CONTENT: std::sync::OnceLock<Result<ContentCollection, ruvane::ContentError>> =
    std::sync::OnceLock::new();
const SOURCES: &[ContentSource<'static>] = &[
    ContentSource::new(
        "01-hello-rust.md",
        include_str!("../content/01-hello-rust.md"),
    ),
    ContentSource::new(
        "02-typed-urls.md",
        include_str!("../content/02-typed-urls.md"),
    ),
    ContentSource::new(
        "03-summer-rust.md",
        include_str!("../content/03-summer-rust.md"),
    ),
    ContentSource::new("04-escaping.md", include_str!("../content/04-escaping.md")),
    ContentSource::new(
        "05-bounded-server.md",
        include_str!("../content/05-bounded-server.md"),
    ),
];

fn content() -> Result<&'static ContentCollection, ruvane::ContentError> {
    CONTENT
        .get_or_init(|| ContentCollection::new(SOURCES))
        .as_ref()
        .map_err(Clone::clone)
}

fn articles() -> &'static ContentCollection {
    content().expect("collection validated before building the application")
}

fn legacy_slug(article: &ContentEntry) -> &str {
    match article.slug().as_str() {
        "summer-rust" => "summer-🦀",
        slug => slug,
    }
}

fn internal(path: &str) -> InternalUrl {
    InternalUrl::new(path).expect("valid fixed example path")
}

fn metadata(description: &str) -> Metadata {
    Metadata::new()
        .description(description)
        .expect("bounded source description")
        .favicon_png(internal("/assets/mark.png"))
}

fn public_metadata(description: &str, origin: Option<&SiteOrigin>, path: &InternalUrl) -> Metadata {
    let metadata = metadata(description);
    match origin {
        Some(origin) => metadata
            .robots(Robots::IndexFollow)
            .canonical(origin.canonical(path)),
        None => metadata.robots(Robots::NoIndexFollow),
    }
}

fn private_metadata(description: &str) -> Metadata {
    metadata(description).robots(Robots::NoIndexFollow)
}

#[derive(Clone, Copy)]
enum Edition {
    Server,
    Static,
}

fn articles_path(edition: Edition) -> &'static str {
    match edition {
        Edition::Server => "/articles",
        Edition::Static => "/articles/",
    }
}

fn named(name: &str) -> PageRoute {
    let pattern = match name {
        "home" => "/",
        "articles" => "/articles/",
        "article-page" => "/articles/page/{page}/",
        "article" => "/articles/{slug}/",
        "about" => "/articles/about/",
        "search" => "/articles",
        "legacy-article" => "/articles/{slug}",
        "legacy-about" => "/articles/about",
        _ => unreachable!("fixed route name"),
    };
    PageRoute::new(name, pattern).expect("fixed named route")
}

fn page(title: &str, body: impl Into<Html>, metadata: Metadata) -> Page {
    Page::new(title, body)
        .metadata(metadata)
        .stylesheet(internal("/assets/site.css"))
}

fn site_layout(context: &PageContext<'_>, title: &str, children: Html) -> Html {
    let legacy = matches!(
        context.route_name(),
        Some("search" | "legacy-article" | "legacy-about")
    );
    let navigation = element(Tag::Nav)
        .attr(Attribute::Class, "site-nav")
        .child(fragment([
            link(&context.url("home", &[]).expect("registered home"), "Home"),
            link(
                &context
                    .url(if legacy { "search" } else { "articles" }, &[])
                    .expect("registered list"),
                "Articles",
            ),
            link(
                &context
                    .url(if legacy { "legacy-about" } else { "about" }, &[])
                    .expect("registered about"),
                "About",
            ),
        ]));
    let brand = element(Tag::Div)
        .attr(Attribute::Class, "brand")
        .child(link(
            &context.url("home", &[]).expect("registered home"),
            fragment([
                image(&internal("/assets/mark.png"), "", 96, 96).expect("fixed dimensions"),
                element(Tag::Span).child("Ruvane journal").render(),
            ]),
        ));
    let header = element(Tag::Header)
        .attr(Attribute::Class, "site-header")
        .child(brand)
        .child(navigation);
    let body = element(Tag::Main)
        .child(
            element(Tag::P)
                .attr(Attribute::Class, "eyebrow")
                .child("The Ruvane journal / Notes on the web"),
        )
        .child(element(Tag::H1).child(title))
        .child(children);
    let footer = element(Tag::Footer)
        .attr(Attribute::Class, "site-footer")
        .child(element(Tag::P).child("Ruvane journal"))
        .child(element(Tag::P).child("Thoughtful software. Room to explore."));
    fragment([header.render(), body.render(), footer.render()])
}

fn journal_layout(_: &PageContext<'_>, _: &str, children: Html) -> Html {
    element(Tag::Section)
        .attr(Attribute::Class, "journal-layout")
        .child(children)
        .render()
}

fn home_body(edition: Edition) -> Html {
    element(Tag::Section)
                .child(element(Tag::P).attr(Attribute::Class, "lead").child("Notes on Rust, thoughtful interfaces, and the craft of building for the web."))
                .child(element(Tag::Div).attr(Attribute::Class, "action").child(link(&internal(articles_path(edition)), "Explore the journal")))
                .child(element(Tag::Div).attr(Attribute::Class, "hero-visual").child(image(&internal("/assets/landscape.png"), "An abstract green landscape under a terracotta sun", 960, 480).expect("fixed dimensions")))
                .child(element(Tag::H2).attr(Attribute::Class, "section-heading").child("Built with care, one page at a time."))
                .child(element(Tag::P).attr(Attribute::Class, "lead").child("Small ideas, shared openly. Follow the links, take your time, and find something worth making.")).render()
}

fn illustration(article: &ContentEntry) -> InternalUrl {
    internal(
        if matches!(legacy_slug(article), "typed-urls" | "escaping") {
            "/assets/geometry.png"
        } else {
            "/assets/landscape.png"
        },
    )
}

fn article_url(article: &ContentEntry, edition: Edition) -> InternalUrl {
    match edition {
        Edition::Server => named("legacy-article").url(&[("slug", legacy_slug(article))]),
        Edition::Static => named("article").url(&[("slug", article.slug().as_str())]),
    }
    .expect("declared article route and source slug")
}

fn listing_url(page: usize, search: &str) -> InternalUrl {
    let url = internal("/articles")
        .query("page", &page.to_string())
        .expect("bounded page number");
    if search.is_empty() {
        url
    } else {
        url.query("search", search)
            .expect("validated query fits the example URL budget")
    }
}

fn article_cards<'a>(articles: impl Iterator<Item = &'a ContentEntry>, edition: Edition) -> Html {
    fragment(articles.map(|article| {
        element(Tag::Li)
            .attr(Attribute::Class, "post-card")
            .child(image(&illustration(article), "", 960, 480).expect("fixed dimensions"))
            .child(
                element(Tag::Div)
                    .attr(Attribute::Class, "card-copy")
                    .child(
                        element(Tag::P)
                            .attr(Attribute::Class, "eyebrow")
                            .child("Field notes"),
                    )
                    .child(
                        element(Tag::H2)
                            .child(link(&article_url(article, edition), article.title())),
                    )
                    .child(element(Tag::P).child(article.description())),
            )
            .render()
    }))
}

fn listing(context: &PageContext<'_>) -> Result<Page, PageError> {
    let request = context.request().ok_or(PageError::InternalServerError)?;
    let origin = context.origin();
    let search = request.query_params().get("search").unwrap_or("");
    let page_number = match request.query_params().get_as::<usize>("page") {
        Ok(value) if value > 0 => value,
        Err(ParamError::Missing) => 1,
        _ => return Ok(invalid_page(search)),
    };
    let needle = search.to_lowercase();
    let articles: Vec<_> = articles()
        .iter()
        .filter(|article| article.title().to_lowercase().contains(&needle))
        .collect();
    let Some(start) = (page_number - 1).checked_mul(PAGE_SIZE) else {
        return Ok(invalid_page(search));
    };
    if page_number > 1 && start >= articles.len() {
        return Err(PageError::NotFound);
    }
    let items = article_cards(
        articles.iter().copied().skip(start).take(PAGE_SIZE),
        Edition::Server,
    );
    let mut body = element(Tag::Section).child(search_panel(search));
    if !search.is_empty() {
        body = body.child(
            element(Tag::P)
                .attr(Attribute::Class, "filter-note")
                .child(format!("Search: {search}")),
        );
    }
    body = body
        .child(
            element(Tag::P)
                .attr(Attribute::Class, "metadata")
                .child(format!("Page {page_number}")),
        )
        .child(
            element(Tag::Ul)
                .attr(Attribute::Class, "post-grid")
                .child(items),
        );
    if articles.is_empty() {
        body = body.child(element(Tag::P).child("No articles found."));
    }
    let mut pagination = element(Tag::Nav).attr(Attribute::Class, "pagination");
    if page_number > 1 {
        pagination = pagination.child(link(&listing_url(page_number - 1, search), "Previous page"));
    }
    if start + PAGE_SIZE < articles.len() {
        pagination = pagination.child(link(&listing_url(page_number + 1, search), "Next page"));
    }
    body = body.child(pagination);
    let metadata = if search.is_empty() {
        let canonical_path = if page_number == 1 {
            internal("/articles")
        } else {
            listing_url(page_number, "")
        };
        public_metadata(
            "Notes on Rust, typed interfaces, and building for the web.",
            origin,
            &canonical_path,
        )
    } else {
        private_metadata("Search the Ruvane journal's articles.")
    };
    Ok(page("Articles", body, metadata))
}

fn article_body(article: &ContentEntry, edition: Edition) -> Html {
    element(Tag::Article)
        .attr(Attribute::Class, "article-body")
        .child(
            element(Tag::Div)
                .attr(Attribute::Class, "hero-visual")
                .child(image(&illustration(article), "", 960, 480).expect("fixed dimensions")),
        )
        .child(article.body().clone())
        .child(
            element(Tag::Nav)
                .attr(Attribute::Class, "pagination")
                .child(link(&internal(articles_path(edition)), "All articles")),
        )
        .render()
}

fn search_panel(value: &str) -> Html {
    let form = GetForm::new(internal("/articles"))
        .expect("query-free internal action")
        .search("search", "Search articles", value)
        .expect("validated query text")
        .hidden("page", "1")
        .expect("unique fixed field")
        .submit_label("Search")
        .expect("fixed label");
    element(Tag::Div)
        .attr(Attribute::Class, "search-panel")
        .child(form)
        .child(
            element(Tag::P)
                .attr(Attribute::Class, "clear-search")
                .child(link(&internal("/articles"), "Clear search")),
        )
        .render()
}

fn invalid_page(search: &str) -> Page {
    page(
        "Articles",
        element(Tag::Section).child(search_panel(search)).child(
            element(Tag::P)
                .attr(Attribute::Class, "form-error")
                .child("That page number is not valid. Try a new search."),
        ),
        private_metadata("The article page number is invalid."),
    )
    .bad_request()
}

const ASSETS: [(&str, &[u8], MediaType); 4] = [
    (
        "/assets/site.css",
        include_bytes!("../assets/site.css"),
        MediaType::Css,
    ),
    (
        "/assets/landscape.png",
        include_bytes!("../assets/landscape.png"),
        MediaType::Png,
    ),
    (
        "/assets/geometry.png",
        include_bytes!("../assets/geometry.png"),
        MediaType::Png,
    ),
    (
        "/assets/mark.png",
        include_bytes!("../assets/mark.png"),
        MediaType::Png,
    ),
];

fn about_body() -> Html {
    element(Tag::P).attr(Attribute::Class, "lead")
        .child("A small publication about clear ideas and readable software. Every article is an invitation to explore how the web works.").render()
}

fn static_listing_url(page: usize) -> InternalUrl {
    if page == 1 {
        named("articles").url(&[])
    } else {
        named("article-page").url(&[("page", &page.to_string())])
    }
    .expect("bounded declared listing route")
}

fn static_listing(context: &PageContext<'_>, number: usize) -> Result<Page, PageError> {
    let count = articles().len().div_ceil(PAGE_SIZE);
    if !(1..=count).contains(&number) {
        return Err(PageError::NotFound);
    }
    let mut pagination = element(Tag::Nav).attr(Attribute::Class, "pagination");
    if number > 1 {
        pagination = pagination.child(link(&static_listing_url(number - 1), "Previous page"));
    }
    if number < count {
        pagination = pagination.child(link(&static_listing_url(number + 1), "Next page"));
    }
    let body = element(Tag::Section)
        .child(
            element(Tag::P)
                .attr(Attribute::Class, "metadata")
                .child(format!("Page {number}")),
        )
        .child(
            element(Tag::Ul)
                .attr(Attribute::Class, "post-grid")
                .child(article_cards(
                    articles()
                        .iter()
                        .skip((number - 1) * PAGE_SIZE)
                        .take(PAGE_SIZE),
                    Edition::Static,
                )),
        )
        .child(pagination);
    Ok(page(
        "Articles",
        body,
        public_metadata(
            "Notes on Rust, typed interfaces, and building for the web.",
            context.origin(),
            context.path(),
        ),
    ))
}

/// Shared pages and assets. Search and legacy Unicode URLs are server-only.
pub fn application() -> Result<App, Box<dyn std::error::Error>> {
    let collection = content()?.clone();
    let mut app = App::new();
    app.layout("site", None, site_layout)?;
    app.layout("journal", Some("site"), journal_layout)?;
    app.page(named("home"), Some("site"), PageMode::fixed(), |context| {
        Ok(page(
            "A small journal. A simpler web.",
            home_body(Edition::Static),
            public_metadata(
                "Notes on Rust, thoughtful interfaces, and the craft of building for the web.",
                context.origin(),
                context.path(),
            ),
        ))
    })?;
    app.page(
        named("articles"),
        Some("journal"),
        PageMode::fixed(),
        |context| static_listing(context, 1),
    )?;
    let pages = (2..=articles().len().div_ceil(PAGE_SIZE))
        .map(|number| PageParams::new(&[("page", &number.to_string())]))
        .collect::<Result<Vec<_>, _>>()?;
    app.page(
        named("article-page"),
        Some("journal"),
        PageMode::Static(pages),
        |context| {
            let number = context
                .param_as::<usize>("page")
                .map_err(|_| PageError::BadRequest)?;
            static_listing(context, number)
        },
    )?;
    app.content_pages(
        named("article"),
        Some("journal"),
        collection,
        |context, article| {
            Ok(page(
                article.title(),
                article_body(article, Edition::Static),
                public_metadata(article.description(), context.origin(), context.path()),
            ))
        },
    )?;
    app.page(
        named("about"),
        Some("journal"),
        PageMode::fixed(),
        |context| {
            Ok(page(
                "About this blog",
                about_body(),
                public_metadata(
                    "A small publication about clear ideas and readable software.",
                    context.origin(),
                    context.path(),
                ),
            ))
        },
    )?;
    app.page(
        named("search"),
        Some("journal"),
        PageMode::ServerOnly(QueryMode::Form),
        listing,
    )?;
    app.page(
        named("legacy-article"),
        Some("journal"),
        PageMode::ServerOnly(QueryMode::Uri),
        |context| {
            let article = articles()
                .iter()
                .find(|a| Some(legacy_slug(a)) == context.param("slug"))
                .ok_or(PageError::NotFound)?;
            Ok(page(
                article.title(),
                article_body(article, Edition::Server),
                public_metadata(
                    article.description(),
                    context.origin(),
                    &article_url(article, Edition::Server),
                ),
            ))
        },
    )?;
    app.page(
        named("legacy-about"),
        Some("journal"),
        PageMode::ServerOnly(QueryMode::Uri),
        |context| {
            Ok(page(
                "About this blog",
                about_body(),
                public_metadata(
                    "A small publication about clear ideas and readable software.",
                    context.origin(),
                    context.path(),
                ),
            ))
        },
    )?;
    for (path, bytes, kind) in ASSETS {
        app.asset(path, bytes, kind)?;
    }
    app.not_found(Some("site"), || {
        page(
            "Not Found",
            element(Tag::P).child("That page is not available."),
            private_metadata("Not Found"),
        )
    })?;
    app.internal_error(Some("site"), || {
        page(
            "Internal Server Error",
            element(Tag::P).child("Please try again later."),
            private_metadata("Internal Server Error"),
        )
    })?;
    Ok(app)
}

/// Builds the example router used by integration tests.
pub fn router() -> Result<Router, Box<dyn std::error::Error>> {
    build_router(None)
}

/// Builds the router with a public origin for canonical URLs.
pub fn router_with_origin(origin: SiteOrigin) -> Result<Router, Box<dyn std::error::Error>> {
    build_router(Some(origin))
}

fn build_router(origin: Option<SiteOrigin>) -> Result<Router, Box<dyn std::error::Error>> {
    let mut router = application()?.router(origin)?;
    router.get("/health", |_| Response::text("ok\n"))?;
    Ok(router)
}

/// Exports the shared pages as a static site.
pub fn static_site(origin: SiteOrigin) -> Result<StaticSite, Box<dyn std::error::Error>> {
    let app = application()?;
    for name in app.server_only_routes() {
        println!("Server-only page omitted from static export: {name}");
    }
    Ok(app.static_site(&origin)?)
}
