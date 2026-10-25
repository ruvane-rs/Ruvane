use ruvane::{
    Attribute, Document, ExportError, GetForm, Html, InternalUrl, MediaType, Metadata, ParamError,
    Request, Response, Robots, Router, SiteOrigin, StaticPath, StaticSite, StatusCode, Tag,
    element, fragment, image, link,
};

const PAGE_SIZE: usize = 2;

struct Article {
    slug: &'static str,
    static_slug: &'static str,
    title: &'static str,
    body: &'static str,
}

const ARTICLES: &[Article] = &[
    Article {
        slug: "bonjour-rust",
        static_slug: "bonjour-rust",
        title: "Hello, Rust & Ruvane",
        body: "A page rendered on the server, built from typed HTML elements.",
    },
    Article {
        slug: "typed-urls",
        static_slug: "typed-urls",
        title: "Typed URLs in Rust",
        body: "Path segments and query values are encoded as data before a link is rendered.",
    },
    Article {
        slug: "été-rust",
        static_slug: "ete-rust",
        title: "Été en Rust",
        body: "UTF-8 slugs travel through percent-encoded URLs and are decoded exactly once.",
    },
    Article {
        slug: "escaping",
        static_slug: "escaping",
        title: "Rendering <text> safely",
        body: "Input such as <script>alert('hello')</script> is displayed as text.",
    },
    Article {
        slug: "bounded-server",
        static_slug: "bounded-server",
        title: "A bounded local server",
        body: "Explicit request limits and deadlines keep the development transport small and inspectable.",
    },
];

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

fn about_path(edition: Edition) -> &'static str {
    match edition {
        Edition::Server => "/articles/about",
        Edition::Static => "/articles/about/",
    }
}

fn page(title: &str, body: impl Into<Html>, metadata: Metadata) -> Response {
    Response::html(render_page(title, body, metadata, Edition::Server))
}

fn render_page(title: &str, body: impl Into<Html>, metadata: Metadata, edition: Edition) -> Html {
    let navigation = element(Tag::Nav)
        .attr(Attribute::Class, "site-nav")
        .child(fragment([
            link(&internal("/"), "Home"),
            link(&internal(articles_path(edition)), "Articles"),
            link(&internal(about_path(edition)), "About"),
        ]));
    let brand = element(Tag::Div)
        .attr(Attribute::Class, "brand")
        .child(link(
            &internal("/"),
            fragment([
                image(&internal("/assets/mark.png"), "", 96, 96).expect("fixed dimensions"),
                element(Tag::Span).child("Ruvane journal").render(),
            ]),
        ));
    let header = element(Tag::Header)
        .attr(Attribute::Class, "site-header")
        .child(brand)
        .child(navigation);
    let layout = element(Tag::Main)
        .child(
            element(Tag::P)
                .attr(Attribute::Class, "eyebrow")
                .child("The Ruvane journal / Notes on the web"),
        )
        .child(element(Tag::H1).child(title))
        .child(body);
    let footer = element(Tag::Footer)
        .attr(Attribute::Class, "site-footer")
        .child(element(Tag::P).child("Ruvane journal"))
        .child(element(Tag::P).child("Thoughtful software. Room to explore."));
    Document::new(
        title,
        fragment([header.render(), layout.render(), footer.render()]),
    )
    .metadata(metadata)
    .stylesheet(internal("/assets/site.css"))
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

fn illustration(article: &Article) -> InternalUrl {
    internal(if matches!(article.slug, "typed-urls" | "escaping") {
        "/assets/geometry.png"
    } else {
        "/assets/landscape.png"
    })
}

fn error(status: StatusCode) -> Response {
    page(
        status.reason(),
        element(Tag::P).child(status.reason()),
        private_metadata(status.reason()),
    )
    .with_status(status)
}

fn article_url(article: &Article, edition: Edition) -> InternalUrl {
    match edition {
        Edition::Server => internal("/articles")
            .segment(article.slug)
            .expect("fixed source slug"),
        Edition::Static => StaticPath::new(&format!("/articles/{}/", article.static_slug))
            .expect("explicit portable source slug")
            .url()
            .clone(),
    }
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

fn article_cards<'a>(articles: impl Iterator<Item = &'a Article>, edition: Edition) -> Html {
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
                        element(Tag::H2).child(link(&article_url(article, edition), article.title)),
                    )
                    .child(element(Tag::P).child(article.body)),
            )
            .render()
    }))
}

fn listing(request: &Request, origin: Option<&SiteOrigin>) -> Response {
    let search = request.query_params().get("search").unwrap_or("");
    let page_number = match request.query_params().get_as::<usize>("page") {
        Ok(value) if value > 0 => value,
        Err(ParamError::Missing) => 1,
        _ => return invalid_page(search),
    };
    let needle = search.to_lowercase();
    let articles: Vec<_> = ARTICLES
        .iter()
        .filter(|article| article.title.to_lowercase().contains(&needle))
        .collect();
    let Some(start) = (page_number - 1).checked_mul(PAGE_SIZE) else {
        return invalid_page(search);
    };
    if page_number > 1 && start >= articles.len() {
        return error(StatusCode::NotFound);
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
    page("Articles", body, metadata)
}

fn article_body(article: &Article, edition: Edition) -> Html {
    element(Tag::Article)
        .attr(Attribute::Class, "article-body")
        .child(
            element(Tag::Div)
                .attr(Attribute::Class, "hero-visual")
                .child(image(&illustration(article), "", 960, 480).expect("fixed dimensions")),
        )
        .child(element(Tag::P).child(article.body))
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

fn invalid_page(search: &str) -> Response {
    page(
        "Articles",
        element(Tag::Section).child(search_panel(search)).child(
            element(Tag::P)
                .attr(Attribute::Class, "form-error")
                .child("That page number is not valid. Try a new search."),
        ),
        private_metadata("The article page number is invalid."),
    )
    .with_status(StatusCode::BadRequest)
}

/// Builds the read-only example blog, shared with the integration tests.
pub fn router() -> Result<Router, Box<dyn std::error::Error>> {
    build_router(None)
}

/// Builds the blog with a trusted public origin for canonical metadata.
pub fn router_with_origin(origin: SiteOrigin) -> Result<Router, Box<dyn std::error::Error>> {
    build_router(Some(origin))
}

fn build_router(origin: Option<SiteOrigin>) -> Result<Router, Box<dyn std::error::Error>> {
    let mut router = Router::new();
    for (path, bytes, kind) in ASSETS {
        router.asset(path, bytes, kind)?;
    }
    // Register the dynamic route first to exercise exact-path precedence.
    let article_origin = origin.clone();
    router.get("/articles/{slug}", move |request| {
        match ARTICLES
            .iter()
            .find(|article| Some(article.slug) == request.param("slug"))
        {
            Some(article) => page(
                article.title,
                article_body(article, Edition::Server),
                public_metadata(
                    article.body,
                    article_origin.as_ref(),
                    &article_url(article, Edition::Server),
                ),
            ),
            None => error(StatusCode::NotFound),
        }
    })?;
    let home_origin = origin.clone();
    router.get("/", move |_| {
        page(
            "A small journal. A simpler web.",
            home_body(Edition::Server),
            public_metadata(
                "Notes on Rust, thoughtful interfaces, and the craft of building for the web.",
                home_origin.as_ref(),
                &internal("/"),
            ),
        )
    })?;
    let listing_origin = origin.clone();
    router.get_form("/articles", move |request| {
        listing(request, listing_origin.as_ref())
    })?;
    router.get("/articles/about", move |_| {
        page(
            "About this blog",
            about_body(),
            public_metadata(
                "A small publication about clear ideas and readable software.",
                origin.as_ref(),
                &internal("/articles/about"),
            ),
        )
    })?;
    router.get("/health", |_| Response::text("ok\n"))?;
    Ok(router)
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

fn static_listing_path(page: usize) -> StaticPath {
    StaticPath::new(&if page == 1 {
        String::from("/articles/")
    } else {
        format!("/articles/page/{page}/")
    })
    .expect("bounded fixed page path")
}

/// Builds the explicit static edition from the same source content and layout.
/// The server edition retains GET search and its original URL conventions.
pub fn static_site(origin: SiteOrigin) -> Result<StaticSite, ExportError> {
    let mut site = StaticSite::new();
    let edition = Edition::Static;
    let home = StaticPath::new("/")?;
    site.page(
        home.clone(),
        render_page(
            "A small journal. A simpler web.",
            home_body(edition),
            public_metadata(
                "Notes on Rust, thoughtful interfaces, and the craft of building for the web.",
                Some(&origin),
                home.url(),
            ),
            edition,
        ),
    )?;
    let page_count = ARTICLES.len().div_ceil(PAGE_SIZE);
    for page_number in 1..=page_count {
        let path = static_listing_path(page_number);
        let mut pagination = element(Tag::Nav).attr(Attribute::Class, "pagination");
        if page_number > 1 {
            pagination = pagination.child(link(
                static_listing_path(page_number - 1).url(),
                "Previous page",
            ));
        }
        if page_number < page_count {
            pagination = pagination.child(link(
                static_listing_path(page_number + 1).url(),
                "Next page",
            ));
        }
        let body = element(Tag::Section)
            .child(
                element(Tag::P)
                    .attr(Attribute::Class, "metadata")
                    .child(format!("Page {page_number}")),
            )
            .child(
                element(Tag::Ul)
                    .attr(Attribute::Class, "post-grid")
                    .child(article_cards(
                        ARTICLES
                            .iter()
                            .skip((page_number - 1) * PAGE_SIZE)
                            .take(PAGE_SIZE),
                        edition,
                    )),
            )
            .child(pagination);
        site.page(
            path.clone(),
            render_page(
                "Articles",
                body,
                public_metadata(
                    "Notes on Rust, typed interfaces, and building for the web.",
                    Some(&origin),
                    path.url(),
                ),
                edition,
            ),
        )?;
    }
    for article in ARTICLES {
        let path = StaticPath::new(article_url(article, edition).as_str())?;
        site.page(
            path.clone(),
            render_page(
                article.title,
                article_body(article, edition),
                public_metadata(article.body, Some(&origin), path.url()),
                edition,
            ),
        )?;
    }
    let about = StaticPath::new(about_path(edition))?;
    site.page(
        about.clone(),
        render_page(
            "About this blog",
            about_body(),
            public_metadata(
                "A small publication about clear ideas and readable software.",
                Some(&origin),
                about.url(),
            ),
            edition,
        ),
    )?;
    for (path, bytes, kind) in ASSETS {
        site.asset(StaticPath::new(path)?, bytes, kind)?;
    }
    Ok(site)
}
