use ruvane::{
    CONTENT_SECURITY_POLICY, Document, ExportError, MAX_STATIC_ENTRIES, MAX_STATIC_FILE_BYTES,
    MAX_STATIC_PATH_BYTES, MAX_STATIC_SEGMENTS, MAX_STATIC_TOTAL_BYTES, MediaType,
    STATIC_COMPLETE_PATH, STATIC_HEADERS_PATH, STATIC_MANIFEST_PATH, StaticPath, StaticSite,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        loop {
            let path = base.join(format!(
                "ruvane-static-test-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("test directory: {error}"),
            }
        }
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn path(value: &str) -> StaticPath {
    StaticPath::new(value).unwrap()
}

fn site() -> StaticSite {
    let mut site = StaticSite::new();
    site.page(
        path("/"),
        Document::new("Été & Rust", "<script> is text").render(),
    )
    .unwrap();
    site.page(
        path("/articles/page/2/"),
        Document::new("Page 2", "Next").render(),
    )
    .unwrap();
    site.asset(
        path("/assets/site.css"),
        b"body { color: #123; }",
        MediaType::Css,
    )
    .unwrap();
    site.asset(
        path("/assets/data.bin"),
        [0, 255, 13, 10, 0],
        MediaType::OctetStream,
    )
    .unwrap();
    site
}

fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, result: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, result);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|part| part.as_os_str().to_str().unwrap())
                    .collect::<Vec<_>>()
                    .join("/");
                result.insert(name, fs::read(path).unwrap());
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn portable_paths_have_explicit_directory_and_file_mappings() {
    for (url, file, page) in [
        ("/", "index.html", true),
        ("/articles/", "articles/index.html", true),
        ("/articles/page/2/", "articles/page/2/index.html", true),
        ("/assets/site.css", "assets/site.css", false),
        ("/assets/a-b_c.min.css", "assets/a-b_c.min.css", false),
    ] {
        let path = path(url);
        assert_eq!(path.url().as_str(), url);
        assert_eq!(path.output_path(), file);
        assert_eq!(path.is_page(), page);
    }
}

#[test]
fn paths_reject_traversal_aliases_unicode_case_and_windows_special_names() {
    for raw in [
        "",
        "relative",
        "../outside",
        "//evil.test/",
        "/a//b/",
        "/./",
        "/../",
        "/a/../b/",
        "/%2e%2e/",
        "/%2Fetc/",
        "/%252F/",
        "/%61/",
        "/A/",
        "/Assets/site.css",
        "/été/",
        "/e\u{301}/",
        "/é.png",
        "/a b/",
        "/a\\b/",
        "/c:/a/",
        "C:\\out",
        "/file.txt:stream",
        "/a?b=1",
        "/a/#x",
        "/a\0/",
        "/a\n/",
        "/a\t/",
        "/_ruvane/",
        "/_ruvane/manifest.tsv",
        "/.hidden/",
        "/a./",
        "/a../",
        "/a.b/",
        "/assets/a..css",
        "/assets/a.css.",
        "/assets/a.css ",
    ] {
        assert_eq!(
            StaticPath::new(raw),
            Err(ExportError::InvalidPath),
            "{raw:?}"
        );
    }
    for name in [
        "con", "prn", "aux", "nul", "com0", "com1", "com9", "lpt0", "lpt1", "lpt9",
    ] {
        for raw in [
            format!("/{name}/"),
            format!("/assets/{name}.txt"),
            format!("/{name}/x.css"),
        ] {
            assert!(StaticPath::new(&raw).is_err(), "{raw}");
        }
    }
}

#[test]
fn path_byte_segment_and_depth_budgets_have_exact_boundaries() {
    let maximum = format!(
        "/{}/{}/{}/{}/",
        "a".repeat(64),
        "b".repeat(64),
        "c".repeat(64),
        "d".repeat(43)
    );
    assert_eq!(maximum.len(), MAX_STATIC_PATH_BYTES);
    assert!(StaticPath::new(&maximum).is_ok());
    let mut oversized = maximum.clone();
    oversized.insert(oversized.len() - 1, 'd');
    assert!(StaticPath::new(&oversized).is_err());
    assert!(StaticPath::new(&format!("/{}/", "x".repeat(64))).is_ok());
    assert!(StaticPath::new(&format!("/{}/", "x".repeat(65))).is_err());
    assert!(StaticPath::new(&format!("/{}.css", "x".repeat(60))).is_ok());
    assert!(StaticPath::new(&format!("/{}.css", "x".repeat(61))).is_err());
    assert!(StaticPath::new(&format!("/{}", "a/".repeat(MAX_STATIC_SEGMENTS))).is_ok());
    assert!(StaticPath::new(&format!("/{}", "a/".repeat(MAX_STATIC_SEGMENTS + 1))).is_err());
}

#[test]
fn assets_require_suffixes_consistent_with_the_fixed_mime_type() {
    let mut site = StaticSite::new();
    for (suffix, kind) in [
        ("txt", MediaType::PlainText),
        ("css", MediaType::Css),
        ("png", MediaType::Png),
        ("jpg", MediaType::Jpeg),
        ("jpeg", MediaType::Jpeg),
        ("webp", MediaType::WebP),
        ("bin", MediaType::OctetStream),
    ] {
        site.asset(path(&format!("/assets/file.{suffix}")), [0, 255], kind)
            .unwrap();
    }
    for suffix in ["html", "svg", "js", "txt", "png"] {
        assert_eq!(
            site.asset(
                path(&format!("/evil.{suffix}")),
                b"<script>x</script>",
                MediaType::Css
            ),
            Err(ExportError::UnsupportedAsset)
        );
    }
    assert_eq!(
        site.page(path("/page.html"), Document::new("x", "").render()),
        Err(ExportError::WrongPathKind)
    );
    assert_eq!(
        site.asset(path("/folder/"), b"x", MediaType::PlainText),
        Err(ExportError::WrongPathKind)
    );
    assert_eq!(site.len(), 7);
}

#[test]
fn duplicate_registration_is_atomic_and_does_not_consume_budgets() {
    let mut site = site();
    let before = site.manifest();
    let bytes = site.planned_bytes();
    assert_eq!(
        site.page(path("/"), Document::new("replacement", "").render()),
        Err(ExportError::DuplicatePath)
    );
    assert_eq!(
        site.asset(path("/assets/site.css"), b"replacement", MediaType::Css),
        Err(ExportError::DuplicatePath)
    );
    assert_eq!(site.manifest(), before);
    assert_eq!(site.planned_bytes(), bytes);
    assert_eq!(site.len(), 4);
}

#[test]
fn entry_count_and_individual_file_limits_are_exact_and_atomic() {
    let mut site = StaticSite::new();
    let full = vec![0; MAX_STATIC_FILE_BYTES];
    site.asset(path("/full.bin"), &full, MediaType::OctetStream)
        .unwrap();
    let before = site.planned_bytes();
    assert_eq!(
        site.asset(
            path("/too-big.bin"),
            vec![0; MAX_STATIC_FILE_BYTES + 1],
            MediaType::OctetStream
        ),
        Err(ExportError::FileTooLarge)
    );
    assert_eq!(site.planned_bytes(), before);
    for index in 1..MAX_STATIC_ENTRIES {
        site.asset(path(&format!("/f{index}.txt")), [], MediaType::PlainText)
            .unwrap();
    }
    assert_eq!(site.len(), MAX_STATIC_ENTRIES);
    assert_eq!(
        site.asset(path("/extra.txt"), [], MediaType::PlainText),
        Err(ExportError::TooManyEntries)
    );
}

#[test]
fn total_byte_limit_includes_metadata_and_rejects_without_mutation() {
    let mut site = StaticSite::new();
    let block = vec![0; MAX_STATIC_FILE_BYTES];
    for index in 0..15 {
        site.asset(
            path(&format!("/f{index}.bin")),
            &block,
            MediaType::OctetStream,
        )
        .unwrap();
    }
    let before = site.planned_bytes();
    let manifest_before = site.manifest();
    assert_eq!(
        site.asset(path("/last.bin"), &block, MediaType::OctetStream),
        Err(ExportError::TotalTooLarge)
    );
    assert_eq!(site.planned_bytes(), before);
    assert_eq!(site.manifest(), manifest_before);
    // The last row length depends on its decimal byte count; solve that small
    // fixed point to exercise the aggregate limit exactly, including metadata.
    let mut size = MAX_STATIC_TOTAL_BYTES - before;
    loop {
        let row = format!("asset\t/last.bin\tlast.bin\tapplication/octet-stream\t{size}\n");
        let next = MAX_STATIC_TOTAL_BYTES - before - row.len();
        if next == size {
            break;
        }
        size = next;
    }
    site.asset(path("/last.bin"), vec![0; size], MediaType::OctetStream)
        .unwrap();
    assert_eq!(site.planned_bytes(), MAX_STATIC_TOTAL_BYTES);
    assert_eq!(
        site.asset(path("/one.txt"), b"x", MediaType::PlainText),
        Err(ExportError::TotalTooLarge)
    );
}

#[test]
fn exports_preserve_bytes_and_manifest_counts_and_finish_with_a_marker() {
    let temp = Temp::new();
    let site = site();
    let destination = temp.0.join("out");
    let report = site.export_to(&destination).unwrap();
    let output = files(&destination);
    assert_eq!(report.files(), 7);
    assert_eq!(output.len(), report.files());
    assert_eq!(output.values().map(Vec::len).sum::<usize>(), report.bytes());
    assert_eq!(report.bytes(), site.planned_bytes());
    assert_eq!(output["assets/data.bin"], [0, 255, 13, 10, 0]);
    assert_eq!(output[STATIC_MANIFEST_PATH], site.manifest().as_bytes());
    assert_eq!(
        output[STATIC_HEADERS_PATH],
        site.required_headers().as_bytes()
    );
    assert!(output[STATIC_COMPLETE_PATH].is_empty());
    let html = std::str::from_utf8(&output["index.html"]).unwrap();
    assert!(html.contains("Été &amp; Rust"));
    assert!(html.contains("&lt;script&gt; is text"));
    assert!(!html.contains("<script>"));
    assert!(site.required_headers().contains(CONTENT_SECURITY_POLICY));
}

#[test]
fn exports_are_identical_across_destinations_and_registration_orders() {
    let temp = Temp::new();
    let mut forward = StaticSite::new();
    let mut reverse = StaticSite::new();
    for index in 0..4 {
        forward
            .page(
                path(&format!("/p{index}/")),
                Document::new(format!("P{index}"), "é🦀").render(),
            )
            .unwrap();
    }
    for index in (0..4).rev() {
        reverse
            .page(
                path(&format!("/p{index}/")),
                Document::new(format!("P{index}"), "é🦀").render(),
            )
            .unwrap();
    }
    forward.export_to(temp.0.join("one")).unwrap();
    reverse.export_to(temp.0.join("two")).unwrap();
    assert_eq!(files(&temp.0.join("one")), files(&temp.0.join("two")));
    assert_eq!(forward.manifest(), reverse.manifest());
    assert!(!forward.manifest().contains(temp.0.to_str().unwrap()));
}

#[test]
fn existing_files_and_directories_are_not_overwritten_or_cleaned() {
    let temp = Temp::new();
    let site = site();
    let directory = temp.0.join("existing");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("sentinel.txt"), b"keep").unwrap();
    assert_eq!(
        site.export_to(&directory),
        Err(ExportError::DestinationExists)
    );
    assert_eq!(fs::read(directory.join("sentinel.txt")).unwrap(), b"keep");
    let file = temp.0.join("file");
    fs::write(&file, b"keep file").unwrap();
    assert_eq!(site.export_to(&file), Err(ExportError::DestinationExists));
    assert_eq!(fs::read(file).unwrap(), b"keep file");
}

#[test]
fn invalid_destinations_and_empty_sites_fail_before_creating_output() {
    let temp = Temp::new();
    let site = site();
    for path in [
        PathBuf::new(),
        temp.0.join("../escape"),
        PathBuf::from(std::path::MAIN_SEPARATOR.to_string()),
    ] {
        assert_eq!(site.export_to(path), Err(ExportError::InvalidDestination));
    }
    assert_eq!(
        site.export_to(temp.0.join("missing/out")),
        Err(ExportError::Io(std::io::ErrorKind::NotFound))
    );
    assert!(!temp.0.join("missing").exists());
    let empty = StaticSite::new();
    assert_eq!(
        empty.export_to(temp.0.join("empty")),
        Err(ExportError::EmptySite)
    );
    assert!(matches!(
        empty.preview_router(),
        Err(ExportError::EmptySite)
    ));
    assert!(!temp.0.join("empty").exists());
}

#[cfg(unix)]
#[test]
fn symlink_destinations_and_parent_aliases_never_modify_the_target() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let site = site();
    let outside = temp.0.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"keep").unwrap();
    let link = temp.0.join("link");
    symlink(&outside, &link).unwrap();
    assert_eq!(site.export_to(&link), Err(ExportError::DestinationExists));
    assert_eq!(site.export_to(link.join("new")), Err(ExportError::Symlink));
    assert!(!outside.join("new").exists());
    let dangling = temp.0.join("dangling");
    symlink(temp.0.join("not-created"), &dangling).unwrap();
    assert_eq!(
        site.export_to(&dangling),
        Err(ExportError::DestinationExists)
    );
    assert!(!temp.0.join("not-created").exists());
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"keep");
}

#[test]
fn deterministic_static_path_mutations_do_not_panic_or_escape_the_profile() {
    let seed = b"/articles/page/2/";
    for position in 0..seed.len() {
        for byte in 0..=127 {
            let mut input = seed.to_vec();
            input[position] = byte;
            if let Ok(path) = StaticPath::new(std::str::from_utf8(&input).unwrap()) {
                assert!(!path.output_path().starts_with('/'));
                assert!(
                    !path
                        .output_path()
                        .split('/')
                        .any(|part| part == "." || part == "..")
                );
                assert_eq!(StaticPath::new(path.url().as_str()).unwrap(), path);
            }
        }
    }
}

#[test]
fn existing_export_preview_loads_without_changing_any_file() {
    let temp = Temp::new();
    let directory = temp.0.join("out");
    site().export_to(&directory).unwrap();
    fs::write(directory.join("unlisted.txt"), b"not a public route").unwrap();
    let before = files(&directory);
    assert!(StaticSite::preview_export(&directory).is_ok());
    assert_eq!(files(&directory), before);
}

#[test]
fn existing_preview_rejects_incomplete_and_inconsistent_exports() {
    let temp = Temp::new();
    for (index, change) in [
        "missing-marker",
        "marker-data",
        "headers",
        "large-manifest",
        "truncated-file",
        "large-file",
        "directory-file",
        "invalid-html-utf8",
    ]
    .iter()
    .enumerate()
    {
        let directory = temp.0.join(format!("case{index}"));
        site().export_to(&directory).unwrap();
        match *change {
            "missing-marker" => fs::remove_file(directory.join(STATIC_COMPLETE_PATH)).unwrap(),
            "marker-data" => fs::write(directory.join(STATIC_COMPLETE_PATH), b"not empty").unwrap(),
            "headers" => fs::write(directory.join(STATIC_HEADERS_PATH), b"no CSP").unwrap(),
            "large-manifest" => fs::write(
                directory.join(STATIC_MANIFEST_PATH),
                vec![b'x'; 160 * 1024 + 1],
            )
            .unwrap(),
            "truncated-file" => fs::write(directory.join("index.html"), b"short").unwrap(),
            "large-file" => fs::write(
                directory.join("assets/data.bin"),
                vec![0; MAX_STATIC_FILE_BYTES + 1],
            )
            .unwrap(),
            "directory-file" => {
                fs::remove_file(directory.join("assets/data.bin")).unwrap();
                fs::create_dir(directory.join("assets/data.bin")).unwrap();
            }
            "invalid-html-utf8" => {
                let mut bytes = fs::read(directory.join("index.html")).unwrap();
                bytes[0] = 255;
                fs::write(directory.join("index.html"), bytes).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(StaticSite::preview_export(&directory).is_err(), "{change}");
    }
}

#[test]
fn existing_preview_validates_all_manifest_rows_before_loading_content() {
    let temp = Temp::new();
    let original = site().manifest();
    let mutations = [
        original.replace("ruvane-static\t1", "ruvane-static\t2"),
        original.replace("\tindex.html\t", "\t../../outside.txt\t"),
        original.replace("\t/articles/page/2/\t", "\t/%2e%2e/\t"),
        original.replace("\t/assets/data.bin\t", "\t/Assets/data.bin\t"),
        original.replace("application/octet-stream", "text/html; charset=utf-8"),
        original.replace("asset\t", "page\t"),
        original.replace("\t5\n", "\t05\n"),
        original.clone() + "asset\t/late.txt\tlate.txt\ttext/plain; charset=utf-8\t1\n",
        original.trim_end().to_owned(),
    ];
    for (index, manifest) in mutations.iter().enumerate() {
        let directory = temp.0.join(format!("case{index}"));
        site().export_to(&directory).unwrap();
        fs::write(directory.join(STATIC_MANIFEST_PATH), manifest).unwrap();
        assert!(
            matches!(
                StaticSite::preview_export(&directory),
                Err(ExportError::InvalidExport)
            ),
            "case {index}"
        );
    }
}

#[cfg(unix)]
#[test]
fn existing_preview_rejects_symlink_roots_files_and_metadata() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let directory = temp.0.join("out");
    site().export_to(&directory).unwrap();
    let alias = temp.0.join("alias");
    symlink(&directory, &alias).unwrap();
    assert!(matches!(
        StaticSite::preview_export(&alias),
        Err(ExportError::Symlink)
    ));
    let target = temp.0.join("target");
    fs::write(&target, [0, 255, 13, 10, 0]).unwrap();
    fs::remove_file(directory.join("assets/data.bin")).unwrap();
    symlink(&target, directory.join("assets/data.bin")).unwrap();
    assert!(matches!(
        StaticSite::preview_export(&directory),
        Err(ExportError::Symlink)
    ));
    fs::remove_file(directory.join("assets/data.bin")).unwrap();
    fs::write(directory.join("assets/data.bin"), [0, 255, 13, 10, 0]).unwrap();
    fs::rename(directory.join("_ruvane"), temp.0.join("metadata")).unwrap();
    symlink(temp.0.join("metadata"), directory.join("_ruvane")).unwrap();
    assert!(matches!(
        StaticSite::preview_export(&directory),
        Err(ExportError::Symlink)
    ));
    assert_eq!(fs::read(target).unwrap(), [0, 255, 13, 10, 0]);
}
