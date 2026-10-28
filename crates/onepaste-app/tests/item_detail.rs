//! The picture of an image item and the whole text of a text item, for the detail pane.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use onepaste_app::*;
use std::sync::Arc;
use std::time::Duration;

const MARKER: &str = "DETAIL-LEAK-MARKER-5521";

async fn app_with_items() -> (App, Arc<FakeClipboard>, Vec<u8>, String) {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("onepaste-detail-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    let clip = Arc::new(FakeClipboard::new());
    let app = App::new(dir, Arc::new(MemorySecretStore::new()), clip.clone(), AppConfig::default());
    app.choose_local_only().await.unwrap();
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    png.extend(MARKER.as_bytes());
    png.extend(std::iter::repeat(3u8).take(400));
    clip.user_copies(Content::Image { png: png.clone(), width: 1280, height: 720 });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let long: String = (1..=40).map(|i| format!("line {i} {MARKER}\n")).collect();
    clip.user_copies(Content::Text(long.clone()));
    tokio::time::sleep(Duration::from_millis(300)).await;
    (app, clip, png, long)
}

async fn ids(app: &App) -> (String, String) {
    let items = app.search("", Filter::All, 10, 0).await.unwrap();
    let image = items.iter().find(|i| i.kind == "image").unwrap().item_id.clone();
    let text = items.iter().find(|i| i.kind == "text").unwrap().item_id.clone();
    (image, text)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_picture_and_the_whole_text_come_back_exactly() {
    let (app, _clip, png, long) = app_with_items().await;
    let (image, text) = ids(&app).await;
    let got = app.item_image(&image).await.unwrap();
    assert_eq!(B64.decode(&got.data).unwrap(), png, "the same bytes that were copied");
    assert_eq!((got.width, got.height, got.size, got.format.as_str()), (1280, 720, png.len(), "png"));
    assert_eq!(app.item_text(&text).await.unwrap(), long, "the whole text with all its lines, not the first line");
    app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn asking_for_the_wrong_kind_or_an_unknown_item_is_a_clear_error() {
    let (app, _clip, _png, _long) = app_with_items().await;
    let (image, text) = ids(&app).await;
    assert_eq!(app.item_image(&text).await.unwrap_err().code(), "not_an_image");
    assert_eq!(app.item_text(&image).await.unwrap_err().code(), "not_text");
    assert_eq!(app.item_image("no-such-item").await.unwrap_err().code(), "not_found");
    assert_eq!(app.item_text("no-such-item").await.unwrap_err().code(), "not_found");
    app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn neither_the_picture_nor_the_text_reaches_the_log_or_the_diagnostics() {
    let (app, _clip, _png, _long) = app_with_items().await;
    let (image, text) = ids(&app).await;
    let _ = app.item_image(&image).await.unwrap();
    let _ = app.item_text(&text).await.unwrap();
    let _ = app.item_image(&text).await;
    let log = serde_json::to_string(&app.event_log()).unwrap();
    let diag = app.diagnostics().await;
    assert!(!log.contains(MARKER) && !diag.contains(MARKER), "a marker planted in the picture and the text must not be in the log or the diagnostics");
    app.shutdown();
}
