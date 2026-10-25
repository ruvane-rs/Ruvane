use ruvane::{
    Attribute, Document, GetForm, Html, InternalUrl, MediaType, ParamError, Request, Response,
    Router, StatusCode, Tag, element, fragment, image, link,
};

const PAGE_SIZE: usize = 2;

struct Article {
    slug: &'static str,
    title: &'static str,
    body: &'static str,
}

const ARTICLES: &[Article] = &[
    Article {
        slug: "bonjour-rust",
        title: "Hello, Rust & Ruvane",
        body: "A page rendered on the server, built from typed HTML elements.",
    },
    Article {
        slug: "typed-urls",
        title: "Typed URLs in Rust",
        body: "Path segments and query values are encoded as data before a link is rendered.",
    },
    Article {
        slug: "été-rust",
        title: "Été en Rust",
        body: "UTF-8 slugs travel through percent-encoded URLs and are decoded exactly once.",
    },
    Article {
        slug: "escaping",
        title: "Rendering <text> safely",
        body: "Input such as <script>alert('hello')</script> is displayed as text.",
    },
    Article {
        slug: "bounded-server",
        title: "A bounded local server",
        body: "Explicit request limits and deadlines keep the development transport small and inspectable.",
    },
];

fn internal(path: &str) -> InternalUrl {
    InternalUrl::new(path).expect("valid fixed example path")
}

fn page(title: &str, body: impl Into<Html>) -> Response {
    let navigation = element(Tag::Nav)
        .attr(Attribute::Class, "site-nav")
        .child(fragment([
            link(&internal("/"), "Home"),
            link(&internal("/articles"), "Articles"),
            link(&internal("/articles/about"), "About"),
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
    Response::html(
        Document::new(
            title,
            fragment([header.render(), layout.render(), footer.render()]),
        )
        .stylesheet(internal("/assets/site.css"))
        .render(),
    )
}

fn illustration(article: &Article) -> InternalUrl {
    internal(if matches!(article.slug, "typed-urls" | "escaping") {
        "/assets/geometry.png"
    } else {
        "/assets/landscape.png"
    })
}

fn error(status: StatusCode) -> Response {
    page(status.reason(), element(Tag::P).child(status.reason())).with_status(status)
}

fn article_url(article: &Article) -> InternalUrl {
    internal("/articles")
        .segment(article.slug)
        .expect("valid fixed example slug")
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

fn listing(request: &Request) -> Response {
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
    let items = fragment(articles.iter().skip(start).take(PAGE_SIZE).map(|article| {
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
                    .child(element(Tag::H2).child(link(&article_url(article), article.title)))
                    .child(element(Tag::P).child(article.body)),
            )
            .render()
    }));
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
    page("Articles", body)
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
    )
    .with_status(StatusCode::BadRequest)
}

/// Builds the read-only example blog, shared with the integration tests.
pub fn router() -> Result<Router, Box<dyn std::error::Error>> {
    let mut router = Router::new();
    router.asset(
        "/assets/site.css",
        include_bytes!("../assets/site.css"),
        MediaType::Css,
    )?;
    router.asset(
        "/assets/landscape.png",
        include_bytes!("../assets/landscape.png"),
        MediaType::Png,
    )?;
    router.asset(
        "/assets/geometry.png",
        include_bytes!("../assets/geometry.png"),
        MediaType::Png,
    )?;
    router.asset(
        "/assets/mark.png",
        include_bytes!("../assets/mark.png"),
        MediaType::Png,
    )?;
    // Register the dynamic route first to exercise exact-path precedence.
    router.get("/articles/{slug}", |request| {
        match ARTICLES
            .iter()
            .find(|article| Some(article.slug) == request.param("slug"))
        {
            Some(article) => page(
                article.title,
                element(Tag::Article)
                    .attr(Attribute::Class, "article-body")
                    .child(
                        element(Tag::Div)
                            .attr(Attribute::Class, "hero-visual")
                            .child(
                                image(&illustration(article), "", 960, 480)
                                    .expect("fixed dimensions"),
                            ),
                    )
                    .child(element(Tag::P).child(article.body))
                    .child(
                        element(Tag::Nav)
                            .attr(Attribute::Class, "pagination")
                            .child(link(&internal("/articles"), "All articles")),
                    ),
            ),
            None => error(StatusCode::NotFound),
        }
    })?;
    router.get("/", |_| {
        page(
            "A small journal. A simpler web.",
            element(Tag::Section)
                .child(element(Tag::P).attr(Attribute::Class, "lead").child("Notes on Rust, thoughtful interfaces, and the craft of building for the web."))
                .child(element(Tag::Div).attr(Attribute::Class, "action").child(link(&internal("/articles"), "Explore the journal")))
                .child(element(Tag::Div).attr(Attribute::Class, "hero-visual").child(image(&internal("/assets/landscape.png"), "An abstract green landscape under a terracotta sun", 960, 480).expect("fixed dimensions")))
                .child(element(Tag::H2).attr(Attribute::Class, "section-heading").child("Built with care, one page at a time."))
                .child(element(Tag::P).attr(Attribute::Class, "lead").child("Small ideas, shared openly. Follow the links, take your time, and find something worth making.")),
        )
    })?;
    router.get_form("/articles", listing)?;
    router.get("/articles/about", |_| page("About this blog", element(Tag::P)
        .attr(Attribute::Class, "lead").child("A small publication about clear ideas and readable software. Every article is an invitation to explore how the web works.")))?;
    router.get("/health", |_| Response::text("ok\n"))?;
    Ok(router)
}
