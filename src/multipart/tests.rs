use super::*;
fn body(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (headers, data) in parts {
        bytes.extend(format!("--test\r\n{headers}\r\n\r\n").as_bytes());
        bytes.extend(*data);
        bytes.extend(b"\r\n");
    }
    bytes.extend(b"--test--\r\n");
    bytes
}
#[test]
fn binary_files_and_raw_text_remain_separate_and_debug_is_redacted() {
    let bytes = body(&[
        (
            "Content-Disposition: form-data; name=title",
            "PRIVATE 🦀+%00\r\n\ttext".as_bytes(),
        ),
        (
            "Content-Disposition: form-data; name=attachment; filename=\"PRIVATE 🦀;\\\"name.txt\"\r\nContent-Type: Application/Octet-Stream",
            b"\0\xff\r\n--testX\r\n--test--X\r\n\x80",
        ),
    ]);
    let data = MultipartData::parse("multipart/form-data; boundary=\"test\"", &bytes).unwrap();
    assert_eq!(data.fields().get("title"), Some("PRIVATE 🦀+%00\n\ttext"));
    let file = data.file().unwrap();
    assert_eq!(file.field_name(), "attachment");
    assert_eq!(file.file_name(), "PRIVATE 🦀;\"name.txt");
    assert_eq!(file.bytes(), b"\0\xff\r\n--testX\r\n--test--X\r\n\x80");
    assert_eq!(file.content_type(), "application/octet-stream");
    assert!(!format!("{data:?} {file:?}").contains("PRIVATE"));
}
fn parse(parts: &[(&str, &[u8])]) -> Result<MultipartData, MultipartError> {
    MultipartData::parse("multipart/form-data; boundary=test", &body(parts))
}
#[test]
fn selected_empty_files_and_unselected_controls_are_distinguishable() {
    for filename in ["", "empty.txt"] {
        let data = parse(&[(
            &format!("Content-Disposition: form-data; name=file; filename=\"{filename}\""),
            b"",
        )])
        .unwrap();
        let file = data.file().unwrap();
        assert!(file.is_empty());
        assert_eq!(file.file_name(), filename);
        assert_eq!(file.content_type(), "application/octet-stream");
    }
    assert_eq!(
        parse(&[(
            "Content-Disposition: form-data; name=file; filename=\"\"",
            b"x"
        )]),
        Err(MultipartError::InvalidFilename)
    );
    let data = parse(&[(
        "Content-Disposition: form-data; name=text\r\nContent-Type: text/plain; charset=\"UTF-8\"",
        b"+%41",
    )])
    .unwrap();
    assert!(data.file().is_none());
    assert_eq!(data.fields().get("text"), Some("+%41"));
}
#[test]
fn boundaries_headers_and_content_types_have_a_closed_profile() {
    for raw in [
        "multipart/form-data",
        "text/plain; boundary=test",
        "multipart/form-data; boundary=",
        "multipart/form-data; boundary=\"unterminated",
        "multipart/form-data; boundary=test; boundary=test",
        "multipart/form-data; boundary=\"with space\"",
        "multipart/form-data; boundary=unquoted=equals",
        "multipart/form-data; charset=UTF-8; boundary=test",
        "multipart/form-data; boundary=\"bad\\quote\"",
        "multipart/form-data; boundary=🦀",
    ] {
        assert!(boundary(raw).is_err(), "{raw}");
    }
    assert!(boundary(&format!("multipart/form-data; boundary={}", "b".repeat(70))).is_ok());
    assert!(boundary(&format!("multipart/form-data; boundary={}", "b".repeat(71))).is_err());
    assert_eq!(
        boundary("Multipart/Form-Data; Boundary=\"a=b?c\""),
        Ok("a=b?c")
    );
    for headers in [
        "Content-Disposition: attachment; name=x",
        "Content-Disposition: form-data; name=x; name=x",
        "Content-Disposition: form-data; name=x; filename=a; filename=b",
        "Content-Disposition: form-data; name=x; filename*=UTF-8''x",
        "Content-Disposition : form-data; name=x",
        "Content-Disposition: form-data; name=x\nContent-Type: text/plain",
        "Content-Disposition: form-data; name=x\r\n folded",
        "Content-Disposition: form-data; name=x\r\nContent-Disposition: form-data; name=x",
        "Content-Disposition: form-data; name=x\r\nContent-Transfer-Encoding: base64",
        "Content-Disposition: form-data; name=x\r\nX-Private: VALUE",
        "Content-Disposition: form-data; name=x; filename=x\r\nContent-Type: multipart/mixed",
        "Content-Disposition: form-data; name=x\r\nContent-Type: text/plain; charset=\"UTF-8",
        "Content-Disposition: form-data; name=x\r\nContent-Type: text/plain\r\nContent-Type: text/plain",
    ] {
        assert!(parse(&[(headers, b"x")]).is_err(), "{headers}");
    }
}
#[test]
fn collisions_unsafe_names_and_invalid_text_fail_without_reinterpreting_bytes() {
    let text = "Content-Disposition: form-data; name=x";
    let file = "Content-Disposition: form-data; name=x; filename=x.bin";
    assert_eq!(
        parse(&[(text, b"a"), (text, b"b")]),
        Err(MultipartError::Duplicate)
    );
    assert_eq!(
        parse(&[(text, b"a"), (file, b"b")]),
        Err(MultipartError::Duplicate)
    );
    assert_eq!(
        parse(&[(file, b"a"), (text, b"b")]),
        Err(MultipartError::Duplicate)
    );
    assert_eq!(
        parse(&[
            (file, b"a"),
            (
                "Content-Disposition: form-data; name=y; filename=y.bin",
                b"b"
            )
        ]),
        Err(MultipartError::Limit)
    );
    assert!(
        parse(&[(
            "Content-Disposition: form-data; name=_ruvane_csrf; filename=x",
            b"x"
        )])
        .is_err()
    );
    for filename in [
        "../secret",
        "..",
        ".",
        "/tmp/private",
        "C:private",
        "a\\\\b",
        "bad\u{85}",
    ] {
        assert!(
            parse(&[(
                &format!("Content-Disposition: form-data; name=f; filename=\"{filename}\""),
                b"x"
            )])
            .is_err()
        );
    }
    for bytes in [
        &[0u8][..],
        &[255],
        b"a\rb",
        &[0xc0, 0xaf],
        &[0xed, 0xa0, 0x80],
    ] {
        assert!(parse(&[(text, bytes)]).is_err());
    }
    assert!(
        parse(&[(
            "Content-Disposition: form-data; name=\"%5Fruvane_csrf\"",
            b"x"
        )])
        .is_err()
    );
    assert!(parse(&[("Content-Disposition: form-data; name=\"x[]\"", b"x")]).is_err());
}
#[test]
fn framing_truncation_preambles_epilogues_and_near_boundaries_are_checked() {
    let bytes = body(&[(
        "Content-Disposition: form-data; name=f; filename=f.bin",
        b"\xff\0\r\n--test-not-a-boundary",
    )]);
    for size in 0..bytes.len() {
        if size == bytes.len() - 2 {
            continue;
        }
        assert!(
            MultipartData::parse("multipart/form-data; boundary=test", &bytes[..size]).is_err(),
            "prefix {size}"
        );
    }
    let mut trailing = bytes.clone();
    trailing.push(b'x');
    assert!(MultipartData::parse("multipart/form-data; boundary=test", &trailing).is_err());
    let mut preamble = b"preamble".to_vec();
    preamble.extend(bytes);
    assert!(MultipartData::parse("multipart/form-data; boundary=test", &preamble).is_err());
    for input in [
        b"--test\n".as_slice(),
        b"--test\r\n\r\n--test--\r\n",
        b"--test--\r\nmore",
    ] {
        assert!(MultipartData::parse("multipart/form-data; boundary=test", input).is_err());
    }
}
#[test]
fn file_field_part_header_and_aggregate_budgets_are_enforced() {
    let file = "Content-Disposition: form-data; name=f; filename=f.bin";
    assert_eq!(
        parse(&[(file, &vec![255; MAX_UPLOAD_BYTES])])
            .unwrap()
            .file()
            .unwrap()
            .len(),
        MAX_UPLOAD_BYTES
    );
    assert_eq!(
        parse(&[(file, &vec![255; MAX_UPLOAD_BYTES + 1])]),
        Err(MultipartError::Limit)
    );
    assert_eq!(
        parse(&[("Content-Disposition: form-data; name=x", &vec![b'x'; 1025])]),
        Err(MultipartError::Limit)
    );
    let headers = (0..16)
        .map(|i| format!("Content-Disposition: form-data; name=f{i}"))
        .collect::<Vec<_>>();
    let mut fields = headers
        .iter()
        .map(|h| (h.as_str(), b"v".as_slice()))
        .collect::<Vec<_>>();
    assert_eq!(parse(&fields).unwrap().fields().len(), 16);
    fields.push((file, b""));
    assert!(parse(&fields).is_ok());
    fields.push(("Content-Disposition: form-data; name=_ruvane_csrf", b"bad"));
    assert!(parse(&fields).is_ok());
    fields.push(("Content-Disposition: form-data; name=overflow", b"v"));
    assert_eq!(parse(&fields), Err(MultipartError::Limit));
    assert_eq!(
        parse(&[(
            &format!("Content-Disposition:{} form-data; name=x", " ".repeat(2048)),
            b"v"
        )]),
        Err(MultipartError::Limit)
    );
    assert_eq!(
        parse(&[(
            &format!(
                "Content-Disposition: form-data; name=f; filename=\"{}\"",
                "x".repeat(256)
            ),
            b"v"
        )]),
        Err(MultipartError::Limit)
    );
    assert_eq!(
        MultipartData::parse(
            "multipart/form-data; boundary=test",
            &vec![0; MAX_MULTIPART_BYTES + 1]
        ),
        Err(MultipartError::Limit)
    );
    let mut pieces = headers
        .iter()
        .map(|h| {
            (
                h.replace("form-data;", &format!("form-data{};", " ".repeat(850))),
                vec![b'x'; 1024],
            )
        })
        .collect::<Vec<_>>();
    pieces.push((file.into(), vec![0; MAX_UPLOAD_BYTES]));
    let encoded = body(
        &pieces
            .iter()
            .map(|(h, b)| (h.as_str(), b.as_slice()))
            .collect::<Vec<_>>(),
    );
    let padding = MAX_MULTIPART_BYTES - encoded.len();
    assert!(padding < 1900);
    pieces.last_mut().unwrap().0 =
        file.replace("form-data;", &format!("form-data{};", " ".repeat(padding)));
    let exact = body(
        &pieces
            .iter()
            .map(|(h, b)| (h.as_str(), b.as_slice()))
            .collect::<Vec<_>>(),
    );
    assert_eq!(exact.len(), MAX_MULTIPART_BYTES);
    assert!(MultipartData::parse("multipart/form-data; boundary=test", &exact).is_ok());
}
#[test]
fn native_csrf_claim_is_separate_and_duplicates_cannot_hide_behind_files() {
    let token = "a".repeat(64);
    let data = parse(&[
        (
            "Content-Disposition: form-data; name=_ruvane_csrf",
            token.as_bytes(),
        ),
        ("Content-Disposition: form-data; name=caption", b"value"),
    ])
    .unwrap();
    assert_eq!(data.fields().len(), 1);
    assert_eq!(data.fields().get(crate::CSRF_FORM_FIELD), None);
    assert_eq!(data.fields().csrf_claim().unwrap().unwrap().as_str(), token);
    assert!(!format!("{data:?}").contains(&token));
    assert!(data.fields().encode().is_err());
    assert_eq!(
        parse(&[
            ("Content-Disposition: form-data; name=_ruvane_csrf", b"x"),
            ("Content-Disposition: form-data; name=_ruvane_csrf", b"y")
        ]),
        Err(MultipartError::Duplicate)
    );
}
#[test]
fn mutation_corpus_never_panics_or_breaks_success_budgets() {
    let original = body(&[(
        "Content-Disposition: form-data; name=f; filename=f.bin",
        b"\xff\0\r\n--testX",
    )]);
    for index in 0..original.len() {
        for byte in [0, 10, 13, 34, 45, 59, 61, 92, 127, 255] {
            let mut bytes = original.clone();
            bytes[index] = byte;
            if let Ok(data) = MultipartData::parse("multipart/form-data; boundary=test", &bytes) {
                assert!(data.fields().len() <= MAX_MULTIPART_FIELDS);
                if let Some(file) = data.file() {
                    assert!(file.len() <= MAX_UPLOAD_BYTES);
                }
            }
        }
    }
}
