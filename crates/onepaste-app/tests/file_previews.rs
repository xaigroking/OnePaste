use onepaste_app::{preview_document, AppError};

#[test]
fn text_preview_decodes_declared_encodings_and_never_guesses_or_replaces_invalid_bytes() {
    assert_eq!(
        preview_document("sample.txt", b"\xef\xbb\xbfActual UTF-8 text\nsecond line")
            .unwrap()
            .text,
        "Actual UTF-8 text\nsecond line"
    );
    let mut wide = vec![0xff, 0xfe];
    wide.extend(
        "Unicode body: \u{4e2d}\u{6587}"
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    assert!(preview_document("sample.txt", &wide)
        .unwrap()
        .text
        .contains("\u{4e2d}\u{6587}"));
    assert_eq!(
        preview_document("sample.txt", &[0xff, 0x80]).unwrap_err(),
        AppError::Attachment("preview_encoding")
    );
    assert_eq!(
        preview_document("opaque.bin", b"opaque").unwrap_err(),
        AppError::Attachment("preview_unsupported")
    );
    assert_eq!(
        preview_document("large.txt", &vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap_err(),
        AppError::Attachment("preview_limit")
    );
}

// Created from our own text with LibreOffice Writer's Word 97 / OOXML exporters.
#[test]
fn actual_doc_and_docx_samples_expose_the_main_document_body_and_table_text() {
    for (name, bytes) in [
        ("body.doc", include_bytes!("samples/body.doc").as_slice()),
        ("body.docx", include_bytes!("samples/body.docx").as_slice()),
    ] {
        let result = preview_document(name, bytes).unwrap();
        for needle in [
            "Actual document body: Alpha & Beta.",
            "Unicode: λ Ω.",
            "Table cell one",
            "Table cell two",
            "Second page content.",
        ] {
            assert!(result.text.contains(needle), "{name} omitted {needle}");
        }
    }
}

fn package(xml: &[u8], count: usize) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("word/document.xml", options).unwrap();
    zip.write_all(xml).unwrap();
    for index in 1..count {
        zip.start_file(format!("extra/{index}"), options).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
#[test]
fn office_previews_reject_expansion_bombs_external_entities_corruption_and_encryption() {
    let oversized = package(&vec![b' '; 8 * 1024 * 1024 + 1], 1);
    assert_eq!(
        preview_document("bomb.docx", &oversized)
            .unwrap_err()
            .code(),
        "preview_limit"
    );
    let entries = package(b"<document><body/></document>", 2049);
    assert_eq!(
        preview_document("entries.docx", &entries)
            .unwrap_err()
            .code(),
        "preview_limit"
    );
    let dtd = package(br#"<!DOCTYPE document [<!ENTITY external SYSTEM "https://example.invalid/never">]><document><body><t>&external;</t></body></document>"#, 1);
    assert_eq!(
        preview_document("external.docx", &dtd).unwrap_err().code(),
        "preview_unsupported"
    );
    let bad = package(b"<document><body><p><t>partial", 1);
    assert_eq!(
        preview_document("broken.docx", &bad).unwrap_err().code(),
        "preview_corrupt"
    );
    for (name, data) in [
        ("body.doc", include_bytes!("samples/body.doc").as_slice()),
        ("body.docx", include_bytes!("samples/body.docx").as_slice()),
    ] {
        assert!(preview_document(name, &data[..data.len() / 2]).is_err());
    }
    use std::io::{Cursor, Read, Seek, SeekFrom, Write};
    let mut ole =
        cfb::CompoundFile::open(Cursor::new(include_bytes!("samples/body.doc").to_vec())).unwrap();
    let mut word = ole.open_stream("/WordDocument").unwrap();
    word.seek(SeekFrom::Start(10)).unwrap();
    let mut flags = [0; 2];
    word.read_exact(&mut flags).unwrap();
    flags[1] |= 1;
    word.seek(SeekFrom::Start(10)).unwrap();
    word.write_all(&flags).unwrap();
    drop(word);
    let encrypted = ole.into_inner().into_inner();
    assert_eq!(
        preview_document("protected.doc", &encrypted)
            .unwrap_err()
            .code(),
        "preview_encrypted"
    );
    assert_eq!(
        preview_document("protected.docx", &encrypted)
            .unwrap_err()
            .code(),
        "preview_encrypted"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn actual_import_and_preview_use_verified_encrypted_cache_and_disappear_when_locked() {
    use onepaste_app::*;
    use std::sync::Arc;
    let root = std::env::temp_dir().join(format!(
        "onepaste-preview-{}",
        onepaste_core::id::new_id().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let dir = root.join("client");
    let app = App::new(
        dir.clone(),
        Arc::new(EncryptedKeyStore::new(dir.clone())),
        Arc::new(FakeClipboard::new()),
        AppConfig {
            background_sync: false,
            listen_for_changes: false,
            ..AppConfig::default()
        },
    );
    app.prepare_local_passphrase("preview sample passphrase")
        .unwrap();
    app.choose_local_only().await.unwrap();
    let mut ids = Vec::new();
    for (name, data) in [
        ("body.doc", include_bytes!("samples/body.doc").as_slice()),
        ("body.docx", include_bytes!("samples/body.docx").as_slice()),
        (
            "body.txt",
            b"Actual document body: Alpha & Beta.".as_slice(),
        ),
        ("body.pdf", include_bytes!("samples/body.pdf").as_slice()),
    ] {
        let path = root.join(name);
        std::fs::write(&path, data).unwrap();
        let id = app.import_file(&path).await.unwrap();
        if name.ends_with("pdf") {
            assert_eq!(app.pdf_preview_bytes(&id).await.unwrap(), data);
        } else {
            assert!(app
                .document_preview(&id)
                .await
                .unwrap()
                .text
                .contains("Actual document body"));
        }
        ids.push(id);
    }
    fn scan(dir: &std::path::Path) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                scan(&p);
            } else {
                assert!(!std::fs::read(&p)
                    .unwrap()
                    .windows(20)
                    .any(|v| v == b"Actual document body"));
            }
        }
    }
    scan(&dir);
    app.lock_session().await.unwrap();
    assert_eq!(
        app.document_preview(&ids[0]).await.unwrap_err(),
        AppError::Locked
    );
    assert_eq!(
        app.pdf_preview_bytes(&ids[3]).await.unwrap_err(),
        AppError::Locked
    );
    app.shutdown();
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
