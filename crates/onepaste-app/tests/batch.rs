//! Batch operations use the real encrypted stores and a fixed ten-second monotonic grace interval.
use onepaste_app::*;
use onepaste_client::{SqliteStore, join, JoinOptions, Session};
use onepaste_core::crypto::Key;
use onepaste_server::{Server, config::Config};
use onepaste_sync::Store;
use std::{future::Future, pin::Pin, path::PathBuf, sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::Duration};

const PASS: &str = "a long batch test passphrase";
#[derive(Default)]
struct Clock(tokio::sync::watch::Sender<Duration>);
impl Clock {
    fn new() -> Self { Self(tokio::sync::watch::channel(Duration::ZERO).0) }
    fn advance(&self, by: Duration) { self.0.send_modify(|now| *now += by); }
}
impl DeletionClock for Clock {
    fn now(&self) -> Duration { *self.0.borrow() }
    fn wait_until(&self, deadline: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let mut receiver = self.0.subscribe();
            loop {
                if *receiver.borrow_and_update() >= deadline { return; }
                if receiver.changed().await.is_err() { return; }
            }
        })
    }
}
fn temp(label: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!("onepaste-batch-{label}-{}-{}",std::process::id(),N.fetch_add(1,Ordering::Relaxed)));
    std::fs::create_dir_all(&path).unwrap(); path
}
fn config(clock: Arc<Clock>) -> AppConfig {
    AppConfig { listen_for_changes: true, background_sync: false, deletion_clock: clock, ..AppConfig::default() }
}
struct Desktop { app: App, dir: PathBuf, secrets: Arc<MemorySecretStore>, clipboard: Arc<FakeClipboard>, clock: Arc<Clock> }
impl Desktop {
    fn new(label: &str) -> Self {
        let dir=temp(label); let secrets=Arc::new(MemorySecretStore::new()); let clipboard=Arc::new(FakeClipboard::new()); let clock=Arc::new(Clock::new());
        let app=App::new(dir.clone(),secrets.clone(),clipboard.clone(),config(clock.clone()));
        Self { app,dir,secrets,clipboard,clock }
    }
    async fn add(&self, text: &str) -> String {
        self.app.on_clip_event(ClipEvent::Captured(Content::Text(text.into()))).await;
        self.app.search(text,Filter::All,10,0).await.unwrap()[0].item_id.clone()
    }
    fn snapshot(&self) -> (Vec<onepaste_sync::ItemRow>,Vec<onepaste_sync::OutboxRow>) {
        let mut db=SqliteStore::open(&self.dir.join("onepaste.db")).unwrap(); let mut result=None;
        db.transact(&mut |tx| { result=Some((tx.items()?,tx.outbox()?)); Ok(()) }).unwrap(); result.unwrap()
    }
    fn restart(&self) -> App { App::new(self.dir.clone(),self.secrets.clone(),self.clipboard.clone(),config(Arc::new(Clock::new()))) }
}
async fn finished(app: &App) {
    tokio::time::timeout(Duration::from_secs(5),async {
        while !app.pending_deletions().pending.is_empty() { tokio::task::yield_now().await; }
    }).await.unwrap();
}

#[tokio::test(flavor="multi_thread")]
async fn batch_copy_preserves_full_text_order_skips_images_and_does_not_create_history() {
    let d=Desktop::new("copy"); d.app.choose_local_only().await.unwrap();
    let a=d.add("first line\nsecond line").await; let b=d.add("last text").await;
    d.app.on_clip_event(ClipEvent::Captured(Content::Image { png: vec![1,2,3], width:1,height:1 })).await;
    let image=d.app.search("",Filter::Images,10,0).await.unwrap()[0].item_id.clone();
    let before=d.snapshot();
    assert_eq!(d.app.copy_items(&[b.clone(),image.clone(),a.clone(),b.clone()]).await.unwrap(),2);
    let writes=d.clipboard.writes(); assert_eq!(writes.len(),1); assert!(writes[0].1);
    assert!(matches!(&writes[0].0,Content::Text(value) if value=="last text\n\nfirst line\nsecond line"));
    tokio::time::timeout(Duration::from_secs(5),async { while !d.app.event_log().iter().any(|entry|entry.kind=="skipped_self") { tokio::task::yield_now().await; } }).await.unwrap();
    assert_eq!(d.snapshot(),before);
    assert!(matches!(d.app.copy_items(&[image]).await,Err(AppError::NotText)));
    assert!(matches!(d.app.copy_items(&[a,"missing".into()]).await,Err(AppError::NotFound)));
    assert_eq!(d.clipboard.writes().len(),1); assert_eq!(d.snapshot(),before); d.app.shutdown();
}

#[tokio::test(flavor="multi_thread")]
async fn favorites_validate_every_id_before_writing_and_local_batches_never_queue_sync() {
    let d=Desktop::new("favorites"); d.app.choose_local_only().await.unwrap(); let a=d.add("alpha").await; let b=d.add("beta").await;
    let before=d.snapshot(); assert!(d.app.favorite_items(&[a.clone(),"missing".into()],true).await.is_err()); assert_eq!(d.snapshot(),before);
    d.app.favorite_items(&[a.clone(),b.clone()],true).await.unwrap(); assert_eq!(d.app.search("",Filter::Favorites,10,0).await.unwrap().len(),2);
    d.app.favorite_items(&[a,b],false).await.unwrap(); assert!(d.app.search("",Filter::Favorites,10,0).await.unwrap().is_empty()); assert!(d.snapshot().1.is_empty()); d.app.shutdown();
}

#[tokio::test(flavor="multi_thread")]
async fn undo_and_expiry_preserve_the_real_synced_outbox_until_exactly_ten_seconds() {
    let dir=temp("server"); let mut cfg=Config::new(dir); let port=std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port(); cfg.listen=format!("127.0.0.1:{port}").parse().unwrap(); cfg.public_url=Some(format!("https://127.0.0.1:{port}"));
    let server=Server::start(cfg).await.unwrap();
    let cli_dir=temp("peer"); join(&cli_dir,JoinOptions {code:server.first_connect_code.clone().unwrap(), device_name:"peer".into(),passphrase:Some(PASS.into()), recovery_kit_out:None,existing_only:false,recovery_kit:None}).await.unwrap();
    let mut peer=Session::open(&cli_dir,PASS).unwrap(); let d=Desktop::new("synced"); d.app.join(&peer.invite().await.unwrap(),PASS,"desktop").await.unwrap();
    let a=d.add("alpha").await; let b=d.add("beta").await; d.app.sync_now().await.unwrap(); peer.engine.request_pull(); peer.run(false).await.unwrap(); assert_eq!(peer.engine.list_items().unwrap().len(),2);
    let before=d.snapshot(); let staged=d.app.stage_deletion(&[a.clone(),b.clone()]).await.unwrap();
    assert_eq!(staged.remaining_ms,10_000); assert_eq!(d.snapshot(),before); assert_eq!(d.app.search("",Filter::All,10,0).await.unwrap().len(),2);
    d.clock.advance(Duration::from_millis(9_999)); tokio::task::yield_now().await; assert_eq!(d.snapshot(),before); assert!(d.app.undo_deletion(&staged.token));
    d.clock.advance(Duration::from_secs(20)); tokio::task::yield_now().await; assert_eq!(d.snapshot(),before);
    let staged=d.app.stage_deletion(&[a,b]).await.unwrap(); d.clock.advance(Duration::from_millis(9_999)); tokio::task::yield_now().await; assert_eq!(d.snapshot(),before);
    d.clock.advance(Duration::from_millis(1)); assert!(!d.app.undo_deletion(&staged.token)); finished(&d.app).await;
    let after=d.snapshot(); assert_eq!(after.0.len(),2); assert!(after.0.iter().all(|row|row.deleted)); assert_eq!(after.1.len(),2);
    d.app.sync_now().await.unwrap(); peer.engine.request_pull(); peer.run(false).await.unwrap(); assert!(peer.engine.list_items().unwrap().iter().all(|row|row.deleted));
    d.app.shutdown(); server.shutdown().await;
}

#[tokio::test(flavor="multi_thread")]
async fn shutdown_cancels_pending_deletion_without_persisting_an_intent() {
    let d=Desktop::new("shutdown"); d.app.choose_local_only().await.unwrap(); let id=d.add("keep after quit").await; let before=d.snapshot();
    d.app.stage_deletion(&[id]).await.unwrap(); d.app.shutdown(); d.clock.advance(Duration::from_secs(60)); tokio::task::yield_now().await;
    let again=d.restart(); again.start().await.unwrap(); assert!(again.pending_deletions().pending.is_empty()); assert_eq!(d.snapshot(),before); again.shutdown();
}

// The child receives only fixture data through stdin, never keys in arguments, environment variables or log output.
#[test]
fn pending_delete_child() {
    if std::env::var_os("ONEPASTE_PENDING_DELETE_CHILD").is_none() { return; }
    use std::io::{Read,Write};
    let mut input=String::new(); std::io::stdin().read_to_string(&mut input).unwrap();
    let (dir,bytes,ids):(PathBuf,[u8;32],Vec<String>)=serde_json::from_str(&input).unwrap();
    let runtime=tokio::runtime::Runtime::new().unwrap(); runtime.block_on(async {
        let secrets=Arc::new(MemorySecretStore::new()); secrets.save(&Key::from_bytes(bytes)).unwrap();
        let app=App::new(dir,secrets,Arc::new(FakeClipboard::new()),config(Arc::new(Clock::new()))); app.start().await.unwrap(); app.stage_deletion(&ids).await.unwrap();
        println!("READY"); std::io::stdout().flush().unwrap(); std::future::pending::<()>().await;
    });
}

#[tokio::test(flavor="multi_thread")]
async fn killing_a_process_during_grace_leaves_no_tombstone_or_outbound_delete_after_restart() {
    use std::{io::{BufRead,BufReader,Write},process::{Command,Stdio}};
    struct Child(std::process::Child); impl Drop for Child { fn drop(&mut self) { let _=self.0.kill(); let _=self.0.wait(); } }
    let d=Desktop::new("crash"); d.app.choose_local_only().await.unwrap(); let id=d.add("keep after crash").await; d.app.shutdown(); let before=d.snapshot();
    let bytes=*d.secrets.load().unwrap().unwrap().as_bytes();
    let mut child=Child(Command::new(std::env::current_exe().unwrap()).args(["--exact","pending_delete_child","--nocapture"]).env("ONEPASTE_PENDING_DELETE_CHILD","1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap());
    let payload=serde_json::to_vec(&(d.dir.clone(),bytes,vec![id])).unwrap(); child.0.stdin.take().unwrap().write_all(&payload).unwrap();
    let stdout=child.0.stdout.take().unwrap(); let (send,recv)=std::sync::mpsc::channel();
    let reader=std::thread::spawn(move || { for line in BufReader::new(stdout).lines() { if line.unwrap()=="READY" { let _=send.send(()); break; } } });
    recv.recv_timeout(Duration::from_secs(20)).unwrap(); assert_eq!(d.snapshot(),before); drop(child); reader.join().unwrap();
    let again=d.restart(); again.start().await.unwrap(); assert!(again.pending_deletions().pending.is_empty()); assert_eq!(d.snapshot(),before); assert_eq!(again.search("",Filter::All,10,0).await.unwrap().len(),1); again.shutdown();
}
