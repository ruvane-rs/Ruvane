use ruvane::{
    ContentBundle, ContentCollection, ContentErrorKind as Kind, ContentSlug, ContentSource,
    MAX_CONTENT_FILE_BYTES, MAX_CONTENT_FILES, MAX_CONTENT_LINE_BYTES, MAX_CONTENT_LINES,
    MAX_CONTENT_TOTAL_BYTES, markdown,
};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

fn document(slug: &str, body: &str) -> String {
    format!("---ruvane\ntitle: A title\nslug: {slug}\ndescription: A description.\n---\n{body}")
}
fn collection(text: &str) -> Result<ContentCollection, ruvane::ContentError> {
    ContentCollection::new(&[ContentSource::new("article.md", text)])
}

#[test]
fn strict_headers_produce_typed_metadata_and_sorted_unique_collections() {
    let first = document("first", "First **body**.");
    let second = document("second", "Second article.");
    let collection = ContentCollection::new(&[
        ContentSource::new("02-second.md", &second),
        ContentSource::new("01-first.md", &first),
    ])
    .unwrap();
    assert_eq!(collection.len(), 2);
    assert!(!collection.is_empty());
    assert_eq!(
        collection
            .iter()
            .map(|e| e.slug().as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    let entry = collection.get("first").unwrap();
    assert_eq!(entry.title(), "A title");
    assert_eq!(entry.description(), "A description.");
    assert_eq!(entry.source_name(), "01-first.md");
    assert_eq!(entry.body().as_str(), "<p>First <strong>body</strong>.</p>");
    assert!(collection.get("../../etc/passwd").is_none());
}

#[test]
fn headers_refuse_missing_unknown_duplicate_and_padded_fields() {
    for text in [
        "# No header",
        "---\ntitle: YAML\n---",
        "---ruvane\ntitle: Only title\n---",
        "---ruvane\ntitle: First\ntitle: Second\n---",
        "---ruvane\nunknown: value\n---",
        "---ruvane\ntitle:  Padded\nslug: slug\ndescription: Text\n---",
        "---ruvane\ntitle: Title\nslug: slug\ndescription: Text",
    ] {
        let error = collection(text).unwrap_err();
        assert_eq!(error.source_name(), Some("article.md"));
        assert!(error.line().unwrap() >= 1);
        assert!(!error.to_string().contains("Padded"));
    }
    assert_eq!(
        collection("---ruvane\ntitle: First\ntitle: Second\n---")
            .unwrap_err()
            .kind(),
        Kind::DuplicateField
    );
    let data = document("first", "body");
    let duplicate = ContentCollection::new(&[
        ContentSource::new("first.md", &data),
        ContentSource::new("second.md", &data),
    ])
    .unwrap_err();
    assert_eq!(duplicate.kind(), Kind::DuplicateSlug);
    assert!(
        ContentCollection::new(&[
            ContentSource::new("first.md", &data),
            ContentSource::new("first.md", &data)
        ])
        .is_err()
    );
}

#[test]
fn names_and_slugs_refuse_traversal_aliases_devices_and_nonportable_forms() {
    let text = document("valid", "body");
    for name in [
        "",
        "../post.md",
        "/post.md",
        "a/b.md",
        "post.MD",
        "λ.md",
        "con.md",
        "nul.md",
        "a.md.exe",
        "a\nb.md",
        "_post.md",
    ] {
        let error = ContentCollection::new(&[ContentSource::new(name, &text)]).unwrap_err();
        assert_eq!(error.kind(), Kind::InvalidName);
        assert_eq!(error.source_name(), None);
        assert!(!error.to_string().contains(name) || name.is_empty());
    }
    for slug in [
        "", ".", "..", "../x", "a/b", "a\\b", "%2e", "🦀", "UPPER", "con", "com1", "_post", "a.b",
        "a?b", "a#b", "a\0b",
    ] {
        assert!(ContentSlug::new(slug).is_err(), "{slug:?}");
        assert!(collection(&document(slug, "body")).is_err());
    }
    assert!(ContentSlug::new(&"a".repeat(64)).is_ok());
    assert!(ContentSlug::new(&"a".repeat(65)).is_err());
}

#[test]
fn markdown_renders_the_documented_typed_blocks_and_inline_markup() {
    let source = "# One\n## Two\n### Three\n#### Four\n##### Five\n###### Six\n\nA **strong** and *emphasized* `code` with [a link](/articles/).\n\n- First\n- Second\n\n1. One\n2. Two\n\n```rust\nlet x = \"<tag>\";\n```\n";
    let html = markdown(source).unwrap();
    assert_eq!(
        html.as_str(),
        "<h1>One</h1><h2>Two</h2><h3>Three</h3><h4>Four</h4><h5>Five</h5><h6>Six</h6><p>A <strong>strong</strong> and <em>emphasized</em> <code>code</code> with <a href=\"/articles/\">a link</a>.</p><ul><li>First</li><li>Second</li></ul><ol><li>One</li><li>Two</li></ol><pre><code class=\"language-rust\">let x = &quot;&lt;tag&gt;&quot;;\n</code></pre>"
    );
    assert_eq!(markdown("a\nb").unwrap().as_str(), "<p>a\nb</p>");
    assert_eq!(
        markdown("\\*literal\\* and [unclosed").unwrap().as_str(),
        "<p>*literal* and [unclosed</p>"
    );
}

#[test]
fn raw_html_mdx_entities_and_code_payloads_remain_escaped_text() {
    let source = "<script>alert('x')</script>\n<img src=x onerror=alert(1)>\n{process.exit()}\n&colon; &#x3c;\n\n```\n</code><script>alert(1)</script>\n```";
    let html = markdown(source).unwrap();
    for forbidden in ["<script", "<img", "</code><script", "&colon;", "&#x3c;"] {
        assert!(!html.as_str().contains(forbidden), "{forbidden}");
    }
    assert!(
        html.as_str()
            .contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;")
    );
    assert!(html.as_str().contains("{process.exit()}"));
    assert!(html.as_str().contains("&amp;colon; &amp;#x3c;"));
}

#[test]
fn markdown_links_use_internal_url_validation_and_html_attribute_escaping() {
    for target in [
        "javascript:alert",
        "data:text/html,hi",
        "https://evil.example/",
        "//evil.example/",
        "/../secret",
        "/%2e%2e/",
        "/%2F/",
        "/%FF/",
        "/x?q=1",
        "/x#f",
        "/a\\b",
        "#anchor",
        "relative",
    ] {
        let error = markdown(&format!("[click]({target})")).unwrap_err();
        assert_eq!(error.kind(), Kind::InvalidLink, "{target}");
    }
    let html =
        markdown("[A <B>](/articles/%F0%9F%A6%80/) and [Q](/%22/) and [percent](/%252F/)").unwrap();
    assert!(html.as_str().contains("A &lt;B&gt;"));
    assert!(html.as_str().contains("href=\"/articles/%F0%9F%A6%80/\""));
    assert!(!html.as_str().contains("javascript:"));
    assert_eq!(
        markdown("![image](/image.png)").unwrap_err().kind(),
        Kind::UnsupportedSyntax
    );
}

#[test]
fn fences_unmatched_markers_and_unsupported_profiles_have_deterministic_results() {
    assert_eq!(
        markdown("```rust\nunclosed").unwrap_err().kind(),
        Kind::UnclosedFence
    );
    assert_eq!(
        markdown("```rust onclick=x\ncode\n```").unwrap_err().kind(),
        Kind::UnsupportedSyntax
    );
    assert_eq!(
        markdown("*unclosed **strong `code").unwrap().as_str(),
        "<p>*unclosed **strong `code</p>"
    );
    assert_eq!(
        markdown("**strong *em* text**").unwrap().as_str(),
        "<p><strong>strong <em>em</em> text</strong></p>"
    );
    assert!(
        markdown("  - Indented list")
            .unwrap()
            .as_str()
            .starts_with("<p>")
    );
}

#[test]
fn diagnostics_name_the_construct_line_without_reflecting_text_or_urls() {
    let text = document(
        "first",
        "First line\nsecond line\n[secret](javascript:secret)\n",
    );
    let error = collection(&text).unwrap_err();
    assert_eq!(error.source_name(), Some("article.md"));
    assert_eq!(error.line(), Some(8));
    assert_eq!(
        error.to_string(),
        "article.md:8: invalid internal Markdown link"
    );
    let error = collection(&document("first", "\n```rust\nsecret")).unwrap_err();
    assert_eq!(error.line(), Some(7));
    assert!(!error.to_string().contains("secret"));
}

#[test]
fn input_line_control_and_collection_budgets_fail_closed() {
    assert_eq!(
        markdown(&"x".repeat(MAX_CONTENT_LINE_BYTES + 1))
            .unwrap_err()
            .kind(),
        Kind::Limit
    );
    assert_eq!(
        markdown(&"x\n".repeat(MAX_CONTENT_LINES))
            .unwrap_err()
            .kind(),
        Kind::Limit
    );
    assert_eq!(
        markdown(&"x".repeat(MAX_CONTENT_FILE_BYTES + 1))
            .unwrap_err()
            .kind(),
        Kind::Limit
    );
    for control in ['\0', '\u{1b}', '\u{7f}', '\u{85}'] {
        assert_eq!(
            markdown(&format!("a{control}b")).unwrap_err().kind(),
            Kind::InvalidText
        );
    }
    assert_eq!(markdown("a\r\nb\r\n").unwrap().as_str(), "<p>a\nb</p>");
    assert!(ContentCollection::new(&[]).is_err());
    let data = document("slug", "body");
    let sources = vec![ContentSource::new("same.md", &data); MAX_CONTENT_FILES + 1];
    assert_eq!(
        ContentCollection::new(&sources).unwrap_err().kind(),
        Kind::Limit
    );
    let body = format!("{}\n", "&".repeat(8000)).repeat(8);
    let owned: Vec<_> = (0..80)
        .map(|i| (format!("p{i}.md"), document(&format!("p{i}"), &body)))
        .collect();
    assert!(owned.iter().map(|(_, text)| text.len()).sum::<usize>() > MAX_CONTENT_TOTAL_BYTES);
    let sources: Vec<_> = owned
        .iter()
        .map(|(name, text)| ContentSource::new(name, text))
        .collect();
    assert_eq!(
        ContentCollection::new(&sources).unwrap_err().kind(),
        Kind::Limit
    );
}

#[test]
fn pathological_unmatched_delimiters_and_nested_inputs_hit_work_budgets() {
    for source in [
        "[".repeat(4000),
        format!("{}x{}", "[".repeat(4000), "](/)".repeat(4000)),
        "*a ".repeat(2500),
    ] {
        match markdown(&source) {
            Ok(html) => assert!(html.as_str().len() <= ruvane::MAX_CONTENT_HTML_BYTES),
            Err(error) => assert!(matches!(
                error.kind(),
                Kind::Complexity | Kind::Limit | Kind::UnsupportedSyntax
            )),
        }
    }
    assert_eq!(
        markdown(&"[".repeat(4000)).unwrap_err().kind(),
        Kind::Complexity
    );
}

#[test]
fn unicode_and_byte_mutations_never_panic_or_create_raw_script_markup() {
    assert_eq!(
        markdown("Rust **日本語** 🌱").unwrap().as_str(),
        "<p>Rust <strong>日本語</strong> 🌱</p>"
    );
    let seed = b"[hello](/articles/) **safe**";
    for i in 0..seed.len() {
        for byte in 0..=127u8 {
            let mut mutated = seed.to_vec();
            mutated[i] = byte;
            let text = String::from_utf8(mutated).unwrap();
            if let Ok(html) = markdown(&text) {
                assert!(!html.as_str().contains("<script"));
            }
        }
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "ruvane-content-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn directory_snapshots_are_sorted_validated_and_independent_of_later_disk_changes() {
    let temp = Temp::new();
    fs::write(temp.0.join("b.md"), document("second", "second")).unwrap();
    fs::write(temp.0.join("a.md"), document("first", "first")).unwrap();
    let bundle = ContentBundle::read_dir(&temp.0).unwrap();
    assert_eq!(
        bundle.sources().map(|s| s.name()).collect::<Vec<_>>(),
        ["a.md", "b.md"]
    );
    let module = bundle.rust_module().unwrap();
    fs::write(temp.0.join("a.md"), b"broken").unwrap();
    assert_eq!(bundle.rust_module().unwrap(), module);
    assert!(ContentBundle::read_dir(&temp.0).is_err());
    assert!(
        ContentCollection::new(&bundle.sources().collect::<Vec<_>>())
            .unwrap()
            .get("first")
            .is_some()
    );
}

#[test]
fn directory_loader_refuses_extra_files_subdirectories_invalid_utf8_and_byte_overflow() {
    for mode in ["extra", "directory", "utf8", "oversize", "count"] {
        let temp = Temp::new();
        match mode {
            "extra" => fs::write(temp.0.join(".env"), b"private").unwrap(),
            "directory" => fs::create_dir(temp.0.join("sub.md")).unwrap(),
            "utf8" => fs::write(temp.0.join("post.md"), [0xff]).unwrap(),
            "oversize" => fs::write(
                temp.0.join("post.md"),
                vec![b'x'; MAX_CONTENT_FILE_BYTES + 1],
            )
            .unwrap(),
            "count" => {
                for i in 0..=MAX_CONTENT_FILES {
                    fs::write(temp.0.join(format!("p{i}.md")), []).unwrap();
                }
            }
            _ => unreachable!(),
        }
        assert!(ContentBundle::read_dir(&temp.0).is_err(), "{mode}");
    }
}

#[cfg(unix)]
#[test]
fn loader_refuses_symlink_roots_parents_files_and_special_files() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let real = temp.0.join("real");
    fs::create_dir(&real).unwrap();
    let post = real.join("post.md");
    fs::write(&post, document("post", "body")).unwrap();
    symlink(&real, temp.0.join("alias")).unwrap();
    assert!(ContentBundle::read_dir(temp.0.join("alias")).is_err());
    let root = temp.0.join("files");
    fs::create_dir(&root).unwrap();
    symlink(&post, root.join("post.md")).unwrap();
    assert!(ContentBundle::read_dir(&root).is_err());
    assert_eq!(fs::read_to_string(post).unwrap(), document("post", "body"));
    fs::remove_file(root.join("post.md")).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(root.join("socket.md")).unwrap();
    assert!(ContentBundle::read_dir(&root).is_err());
    drop(listener);
    fs::create_dir(real.join("nested")).unwrap();
    assert!(ContentBundle::read_dir(temp.0.join("alias/nested")).is_err());
}

#[test]
fn embedded_rust_module_quotes_all_source_as_string_data() {
    let temp = Temp::new();
    let payload = "\"); panic!(\"injected\"); //\ninclude_str!(\"/private/secret\");\n</script>\n";
    fs::write(temp.0.join("payload.md"), document("payload", payload)).unwrap();
    let bundle = ContentBundle::read_dir(&temp.0).unwrap();
    let source = bundle.rust_module().unwrap();
    assert!(source.starts_with("// ruvane-content\t1\n"));
    assert!(source.contains("\\\"); panic!(\\\"injected\\\"); //\\n"));
    assert_eq!(
        source
            .lines()
            .filter(|line| line.contains("ContentSource::new"))
            .count(),
        1
    );
    assert!(
        ContentCollection::new(&bundle.sources().collect::<Vec<_>>())
            .unwrap()
            .get("payload")
            .unwrap()
            .body()
            .as_str()
            .contains("&lt;/script&gt;")
    );
}
