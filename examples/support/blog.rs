use ruvane::{
    Document, Html, InternalUrl, ParamError, Request, Response, RouteError, Router, StatusCode,
    Tag, element, fragment, link, text,
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
    let navigation = element(Tag::Nav).child(fragment([
        link(&internal("/"), "Home"),
        text(" | "),
        link(&internal("/articles"), "Articles"),
        text(" | "),
        link(&internal("/articles/about"), "About"),
    ]));
    let layout = element(Tag::Main)
        .child(navigation)
        .child(element(Tag::H1).child(title))
        .child(body);
    Response::html(Document::new(title, layout).render())
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
    let page_number = match request.query_params().get_as::<usize>("page") {
        Ok(value) if value > 0 => value,
        Err(ParamError::Missing) => 1,
        _ => return error(StatusCode::BadRequest),
    };
    let search = request.query_params().get("search").unwrap_or("");
    let needle = search.to_lowercase();
    let articles: Vec<_> = ARTICLES
        .iter()
        .filter(|article| article.title.to_lowercase().contains(&needle))
        .collect();
    let Some(start) = (page_number - 1).checked_mul(PAGE_SIZE) else {
        return error(StatusCode::BadRequest);
    };
    if page_number > 1 && start >= articles.len() {
        return error(StatusCode::NotFound);
    }
    let items = fragment(articles.iter().skip(start).take(PAGE_SIZE).map(|article| {
        element(Tag::Li)
            .child(link(&article_url(article), article.title))
            .render()
    }));
    let mut body = element(Tag::Section)
        .child(element(Tag::P).child(format!("Search: {search}")))
        .child(element(Tag::P).child(format!("Page {page_number}")))
        .child(element(Tag::Ul).child(items));
    if articles.is_empty() {
        body = body.child(element(Tag::P).child("No articles found."));
    }
    if page_number > 1 {
        body = body.child(link(&listing_url(page_number - 1, search), "Previous page"));
    }
    if start + PAGE_SIZE < articles.len() {
        body = body
            .child(text(" "))
            .child(link(&listing_url(page_number + 1, search), "Next page"));
    }
    page("Articles", body)
}

/// Builds the read-only example blog, shared with the integration tests.
pub fn router() -> Result<Router, RouteError> {
    let mut router = Router::new();
    // Register the dynamic route first to exercise exact-path precedence.
    router.get("/articles/{slug}", |request| {
        match ARTICLES
            .iter()
            .find(|article| Some(article.slug) == request.param("slug"))
        {
            Some(article) => page(
                article.title,
                element(Tag::Article).child(element(Tag::P).child(article.body)),
            ),
            None => error(StatusCode::NotFound),
        }
    })?;
    router.get("/", |_| {
        page(
            "Ruvane blog",
            element(Tag::P)
                .child("A small site rendered entirely in Rust. ")
                .child(link(&internal("/articles"), "Read the articles")),
        )
    })?;
    router.get("/articles", listing)?;
    router.get("/articles/about", |_| page("About this blog", element(Tag::P)
        .child("Fixed routes take priority over dynamic captures. Articles are stored in this example's source code.")))?;
    router.get("/health", |_| Response::text("ok\n"))?;
    Ok(router)
}
