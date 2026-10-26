use ruvane::{
    App, FileLayout, FilePage, FileRouteRegistry, FileRoutes, Page, PageMode, PageParams,
    SiteOrigin, Tag, element,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "ruvane file routes \"{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, relative: &str, body: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }
    fn routes(&self) -> Result<FileRoutes, ruvane::FileRouteError> {
        FileRoutes::read_dir(self.0.join("routes"))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn discovery_is_deterministic_and_generates_typed_imports_with_escaped_paths() {
    let temp = Temp::new();
    for file in [
        "routes/tasks/[id]/page.rs",
        "routes/tasks/settings/page.rs",
        "routes/tasks/layout.rs",
        "routes/page.rs",
        "routes/layout.rs",
    ] {
        temp.file(file, "// trusted source\n");
    }
    let routes = temp.routes().unwrap();
    assert_eq!(routes.page_count(), 3);
    assert_eq!(routes.layout_count(), 2);
    let source = routes.rust_module().unwrap();
    assert_eq!(source, temp.routes().unwrap().rust_module().unwrap());
    assert!(source.contains("registry.page(\"tasks/[id]/page.rs\", \"/tasks/{id}\""));
    assert!(source.contains("registry.layout(\"tasks/layout.rs\", \"/tasks\", Some(&layout_0)"));
    assert!(
        source.contains("\\\"") && source.contains("::page()") && source.contains("::layout()")
    );
    assert!(source.ends_with("    registry.finish()\n}\n"));
}

#[test]
fn ambiguous_patterns_duplicate_captures_and_unsupported_entries_have_source_locations() {
    let temp = Temp::new();
    temp.file("routes/tasks/[id]/page.rs", "");
    temp.file("routes/tasks/[slug]/page.rs", "");
    let error = temp.routes().err().unwrap();
    assert!(error.to_string().contains("ambiguous"));
    assert!(error.path().contains("page.rs"));
    for file in [
        "routes/tasks/[id]/[id]/page.rs",
        "routes/(group)/page.rs",
        "routes/UPPER/page.rs",
        "routes/[...parts]/page.rs",
        "routes/ruvane-runtime/page.rs",
        "routes/unexpected.rs",
        "routes/notes.md",
    ] {
        let temp = Temp::new();
        temp.file("routes/page.rs", "");
        temp.file(file, "");
        assert!(temp.routes().is_err(), "{file}");
    }
    let temp = Temp::new();
    temp.file("routes/alone/layout.rs", "");
    assert!(
        temp.routes()
            .unwrap_err()
            .to_string()
            .contains("layout has no descendant")
    );
}

#[test]
fn directory_source_and_module_budgets_fail_closed() {
    let temp = Temp::new();
    temp.file(
        "routes/page.rs",
        vec![b' '; ruvane::MAX_FILE_ROUTE_SOURCE_BYTES + 1],
    );
    assert!(
        temp.routes()
            .unwrap_err()
            .to_string()
            .contains("byte limit")
    );
    let temp = Temp::new();
    temp.file("routes/page.rs", [255]);
    assert!(temp.routes().unwrap_err().to_string().contains("UTF-8"));
    let temp = Temp::new();
    temp.file("routes/page.rs", "");
    temp.file(&format!("routes/{}page.rs", "a/".repeat(17)), "");
    assert!(
        temp.routes()
            .unwrap_err()
            .to_string()
            .contains("directory budget")
    );
    let temp = Temp::new();
    for index in 0..17 {
        temp.file(
            &format!("routes/p{index}/page.rs"),
            vec![b' '; ruvane::MAX_FILE_ROUTE_SOURCE_BYTES],
        );
    }
    assert!(
        temp.routes()
            .unwrap_err()
            .to_string()
            .contains("total byte")
    );
    let temp = Temp::new();
    temp.file("routes/layout.rs", "");
    temp.file("routes/page.rs", "");
    for index in 0..127 {
        temp.file(&format!("routes/p{index}/page.rs"), "");
    }
    assert!(
        temp.routes()
            .unwrap_err()
            .to_string()
            .contains("source count")
    );
    let temp = Temp::new();
    temp.file("routes/page.rs", "");
    for index in 0..513 {
        fs::create_dir(temp.0.join(format!("routes/p{index}"))).unwrap();
    }
    assert!(
        temp.routes()
            .unwrap_err()
            .to_string()
            .contains("entry limit")
    );
    assert!(FileRoutes::read_dir(Path::new("src/../src")).is_err());
}

#[test]
fn output_publication_preserves_foreign_files_and_recognizes_its_own_marker() {
    let temp = Temp::new();
    temp.file("routes/page.rs", "");
    let output = temp.file("output/module.rs", b"keep");
    let foreign = temp.file("output/foreign", b"untouched");
    let routes = temp.routes().unwrap();
    assert!(routes.write_module(&output).is_err());
    assert_eq!(fs::read(&output).unwrap(), b"keep");
    fs::remove_file(&output).unwrap();
    routes.write_module(&output).unwrap();
    routes.write_module(&output).unwrap();
    assert_eq!(
        fs::read_to_string(&output).unwrap(),
        routes.rust_module().unwrap()
    );
    assert_eq!(fs::read(foreign).unwrap(), b"untouched");
    assert!(
        !fs::read_dir(output.parent().unwrap())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
    );
}

#[test]
fn registry_commits_native_layout_chains_and_does_not_run_loaders_during_registration() {
    let mut app = App::new();
    let loads = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&loads);
    let mut registry = FileRouteRegistry::new(&mut app);
    let root = registry
        .layout(
            "layout.rs",
            "/",
            None,
            FileLayout::server("site", |_, _, children| {
                element(Tag::Main).child(children).render()
            }),
        )
        .unwrap();
    let journal = registry
        .layout(
            "journal/layout.rs",
            "/journal",
            Some(&root),
            FileLayout::server("journal", |_, _, children| {
                element(Tag::Section).child(children).render()
            }),
        )
        .unwrap();
    registry
        .page(
            "journal/[slug]/page.rs",
            "/journal/{slug}",
            Some(&journal),
            FilePage::server(
                "article",
                PageMode::Static(vec![PageParams::new(&[("slug", "hello")]).unwrap()]),
                move |context| {
                    counted.fetch_add(1, Ordering::Relaxed);
                    Ok(Page::new("Article", context.param("slug").unwrap()))
                },
            ),
        )
        .unwrap();
    registry.finish().unwrap();
    assert_eq!(loads.load(Ordering::Relaxed), 0);
    assert_eq!(
        app.url("article", &[("slug", "hello")]).unwrap().as_str(),
        "/journal/hello/"
    );
    let temp = Temp::new();
    app.static_site(&SiteOrigin::parse("https://example.com").unwrap())
        .unwrap()
        .export_to(temp.0.join("site"))
        .unwrap();
    assert_eq!(loads.load(Ordering::Relaxed), 1);
    let html = fs::read_to_string(temp.0.join("site/journal/hello/index.html")).unwrap();
    assert!(html.contains("<main><section>hello</section></main>"));
}

#[test]
fn failed_or_abandoned_registries_never_modify_the_original_app() {
    let mut app = App::new();
    {
        let mut registry = FileRouteRegistry::new(&mut app);
        registry
            .page(
                "page.rs",
                "/",
                None,
                FilePage::server("home", PageMode::fixed(), |_| Ok(Page::new("Home", "Home"))),
            )
            .unwrap();
        let error = registry
            .page(
                "tasks/page.rs",
                "/tasks",
                None,
                FilePage::server("home", PageMode::fixed(), |_| {
                    Ok(Page::new("Tasks", "Tasks"))
                }),
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("tasks/page.rs") && error.to_string().contains("duplicate")
        );
        assert!(registry.finish().is_err());
    }
    assert!(app.url("home", &[]).is_err());
    {
        let mut registry = FileRouteRegistry::new(&mut app);
        registry
            .page(
                "page.rs",
                "/",
                None,
                FilePage::server("home", PageMode::fixed(), |_| Ok(Page::new("Home", "Home"))),
            )
            .unwrap();
    }
    assert!(app.url("home", &[]).is_err());
}

#[test]
fn native_and_client_layout_kinds_cannot_be_silently_mixed() {
    let mut app = App::new();
    let mut registry = FileRouteRegistry::new(&mut app);
    let layout = registry
        .layout(
            "layout.rs",
            "/",
            None,
            FileLayout::server("native", |_, _, body| body),
        )
        .unwrap();
    let page = FilePage::client(
        "home",
        1,
        |_| Ok(ruvane::InitialState::Number(0)),
        |_| {
            Ok(ruvane::ClientPageData::new(
                "Home",
                ruvane::ActionData::default(),
            ))
        },
    );
    assert!(
        registry
            .page("page.rs", "/", Some(&layout), page)
            .unwrap_err()
            .to_string()
            .contains("kinds do not match")
    );
    assert!(registry.finish().is_err());
    assert!(app.url("home", &[]).is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_are_rejected_in_roots_sources_ancestors_and_outputs() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let source = temp.file("routes/page.rs", "");
    symlink(temp.0.join("routes"), temp.0.join("linked")).unwrap();
    assert!(FileRoutes::read_dir(temp.0.join("linked")).is_err());
    let routes = temp.routes().unwrap();
    let output = temp.file("out/module.rs", b"keep");
    symlink(&output, temp.0.join("out/link.rs")).unwrap();
    assert!(routes.write_module(temp.0.join("out/link.rs")).is_err());
    symlink(temp.0.join("out"), temp.0.join("linked-out")).unwrap();
    assert!(
        routes
            .write_module(temp.0.join("linked-out/module.rs"))
            .is_err()
    );
    fs::remove_file(&source).unwrap();
    symlink(&output, &source).unwrap();
    assert!(temp.routes().is_err());
    assert!(routes.rust_module().is_err());
    assert_eq!(fs::read(&output).unwrap(), b"keep");
}

#[test]
fn screen_sources_are_typed_inherited_and_counted_in_the_same_scan_budget() {
    let temp = Temp::new();
    for file in [
        "routes/tasks/loading.rs",
        "routes/tasks/error.rs",
        "routes/tasks/layout.rs",
        "routes/tasks/page.rs",
        "routes/tasks/[id]/error.rs",
        "routes/tasks/[id]/page.rs",
    ] {
        temp.file(file, "");
    }
    let routes = temp.routes().unwrap();
    assert_eq!(
        (
            routes.page_count(),
            routes.layout_count(),
            routes.boundary_count()
        ),
        (2, 1, 2)
    );
    let source = routes.rust_module().unwrap();
    assert!(source.contains("registry.boundary(None, Some((\"tasks/loading.rs\""));
    assert!(
        source.contains("registry.boundary(Some(&boundary_0), None, Some((\"tasks/[id]/error.rs\"")
    );
    assert!(source.contains("::layout().with_boundary(&boundary_0)"));
    assert!(source.contains("::page().with_boundary(&boundary_1)"));
    assert_eq!(source, routes.rust_module().unwrap());
    for file in ["loading.rs", "error.rs"] {
        let temp = Temp::new();
        temp.file("routes/page.rs", "");
        temp.file(&format!("routes/orphan/{file}"), "");
        let error = temp.routes().unwrap_err();
        assert!(error.path().ends_with(file));
        assert!(error.to_string().contains("no descendant pages"));
    }
    let temp = Temp::new();
    temp.file("routes/page.rs", "");
    temp.file("routes/loading.rs", "");
    temp.file(
        "routes/error.rs",
        vec![b' '; ruvane::MAX_FILE_ROUTE_SOURCE_BYTES + 1],
    );
    assert!(temp.routes().unwrap_err().path().ends_with("error.rs"));
}

#[test]
fn regeneration_rechecks_the_combined_source_budget_including_screens() {
    let temp = Temp::new();
    for index in 0..9 {
        temp.file(&format!("routes/p{index}/page.rs"), "");
        temp.file(&format!("routes/p{index}/error.rs"), "");
    }
    let routes = temp.routes().unwrap();
    for index in 0..9 {
        for name in ["page.rs", "error.rs"] {
            temp.file(
                &format!("routes/p{index}/{name}"),
                vec![b' '; ruvane::MAX_FILE_ROUTE_SOURCE_BYTES],
            );
        }
    }
    assert!(
        routes
            .rust_module()
            .unwrap_err()
            .to_string()
            .contains("total byte limit")
    );
}

#[test]
fn not_found_files_have_independent_inheritance_and_share_source_limits() {
    let temp = Temp::new();
    for file in [
        "routes/not_found.rs",
        "routes/page.rs",
        "routes/tasks/not_found.rs",
        "routes/tasks/layout.rs",
        "routes/tasks/page.rs",
        "routes/tasks/[id]/page.rs",
    ] {
        temp.file(file, "");
    }
    let routes = temp.routes().unwrap();
    assert_eq!(routes.not_found_count(), 2);
    let source = routes.rust_module().unwrap();
    assert!(
        source.contains("registry.not_found(\"not_found.rs\", __ruvane_file_0::not_found(), true)")
    );
    assert!(source.contains(
        "registry.not_found(\"tasks/not_found.rs\", __ruvane_file_2::not_found(), false)"
    ));
    assert!(source.contains("::layout().with_not_found(&not_found_2)"));
    assert_eq!(
        source
            .matches("::page().with_not_found(&not_found_2)")
            .count(),
        2
    );
    assert_eq!(source, routes.rust_module().unwrap());
    let temp = Temp::new();
    temp.file("routes/page.rs", "");
    temp.file("routes/orphan/not_found.rs", "");
    assert!(
        temp.routes()
            .unwrap_err()
            .to_string()
            .contains("orphan/not_found.rs: not-found presentation has no descendant pages")
    );
    let temp = Temp::new();
    temp.file("routes/page.rs", "");
    temp.file(
        "routes/not_found.rs",
        vec![b' '; ruvane::MAX_FILE_ROUTE_SOURCE_BYTES + 1],
    );
    assert_eq!(temp.routes().unwrap_err().path(), "not_found.rs");
}
