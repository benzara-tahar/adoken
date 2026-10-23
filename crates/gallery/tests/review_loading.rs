//! Review loading through a real window, headless (spec 0004 States): a PR
//! opened before its file list was synced, over a signed-in org whose
//! Azure DevOps is down, then back. The file list goes loading → failed →
//! (⇧L) → ready; the selected file's blobs then fail → Retry (⇧L) → the
//! diff shows. No load is left spinning after it settles. macOS only, like
//! `snapshots.rs`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("review_loading: skipped (headless rendering is macOS-only for now)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashSet;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use adoken_app::review::blobs::Blobs;
    use adoken_app::review::data::{self, Layout};
    use adoken_app::review::loading::ListLoad;
    use adoken_app::review::pr::{PrReview, RetryLoading};
    use adoken_demo::RICH_PR_IDS;
    use adoken_sync::pr_files::{PrFiles, RepoRef, git_blob_id};
    use adoken_sync::transport::{ApiResponse, RateHeaders, TransportError};
    use adoken_sync::{ApiRequest, Governor, SystemClock, Transport};
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, bail, ensure};
    use gpui::{
        Action, AppContext as _, Focusable as _, HeadlessAppContext, WindowHandle, px, size,
    };

    const OLD: &str = "fn retry() {}\n";
    const NEW: &str = "fn retry() -> u8 { 3 }\n";

    /// Azure DevOps for one PR with one edited file; the list and the blobs
    /// each answer "offline" while their flag is set.
    struct Ado {
        list_down: AtomicBool,
        blobs_down: AtomicBool,
    }

    impl Transport for Ado {
        fn send(
            &self,
            req: ApiRequest,
        ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, TransportError>> + Send + '_>>
        {
            let path = req.url().path().to_owned();
            let (old, new) = (git_blob_id(OLD.as_bytes()), git_blob_id(NEW.as_bytes()));
            let answer = if path.contains("/blobs/") {
                if self.blobs_down.load(Ordering::SeqCst) {
                    Err(TransportError::Offline("network down".into()))
                } else if path.ends_with(&old) {
                    Ok(OLD.as_bytes().to_vec())
                } else {
                    Ok(NEW.as_bytes().to_vec())
                }
            } else if self.list_down.load(Ordering::SeqCst) {
                Err(TransportError::Offline("network down".into()))
            } else if path.ends_with("/iterations") {
                Ok(format!(
                    r#"{{"count":1,"value":[{{"id":1,"sourceRefCommit":{{"commitId":"{new}"}},"commonRefCommit":{{"commitId":"{old}"}}}}]}}"#
                )
                .into_bytes())
            } else {
                Ok(format!(
                    r#"{{"changeEntries":[{{"changeTrackingId":1,"changeType":"edit","item":{{"path":"/src/retry.rs","objectId":"{}","originalObjectId":"{}"}}}}]}}"#,
                    new.to_ascii_uppercase(),
                    old.to_ascii_uppercase()
                )
                .into_bytes())
            };
            Box::pin(async move {
                answer.map(|body| ApiResponse {
                    status: 200,
                    rate: RateHeaders::default(),
                    body,
                    content_type: None,
                    validators: Default::default(),
                })
            })
        }
    }

    pub fn run() -> anyhow::Result<()> {
        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| {
            cx.set_reduce_motion(true);
            let variant = ThemeRegistry::house_variants()
                .next()
                .expect("a house theme");
            Theme::install(variant, cx);
            adoken_gallery::init(cx);
            gpui_tokio::init(cx);
        });
        let dir = tempfile::tempdir()?;
        let ado = Arc::new(Ado {
            list_down: AtomicBool::new(true),
            blobs_down: AtomicBool::new(true),
        });
        let window = open(&mut cx, dir.path(), ado.clone())?;

        // Opened with no stored list: the refresh runs, then fails.
        window.update(&mut cx, |review, _, cx| review.sync_files(cx))?;
        let loading =
            window.update(&mut cx, |review, _, _| review.file_list_load().is_loading())?;
        ensure!(loading, "the file list is loading");
        wait_for(&mut cx, window, "the list to fail", |review| {
            review.file_list_load().failure().is_some()
        })?;
        let empty = window.update(&mut cx, |review, _, _| review.loaded().files.is_empty())?;
        ensure!(empty, "no files yet");

        // Back online for lists (blobs still down): ⇧L refetches it.
        ado.list_down.store(false, Ordering::SeqCst);
        dispatch(&mut cx, window, RetryLoading)?;
        wait_for(
            &mut cx,
            window,
            "the list, then the file's failure",
            |review| {
                *review.file_list_load() == ListLoad::Idle
                    && review.loaded().files.len() == 1
                    && review.loaded().files[0].unavailable.is_some()
            },
        )?;
        let spinning = window.update(&mut cx, |review, _, _| review.is_loading(0))?;
        ensure!(!spinning, "a failed file isn't left loading");

        // Blobs back: ⇧L retries the selected file; its diff shows.
        ado.blobs_down.store(false, Ordering::SeqCst);
        dispatch(&mut cx, window, RetryLoading)?;
        wait_for(&mut cx, window, "the diff", |review| {
            let file = &review.loaded().files[0];
            file.diff.is_some() && file.unavailable.is_none()
        })?;
        let spinning = window.update(&mut cx, |review, _, _| review.is_loading(0))?;
        ensure!(!spinning, "a loaded file isn't left loading");
        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        cx.run_until_parked();
        println!("review_loading: list failed → retried → loaded; file failed → retried → diffed");
        Ok(())
    }

    /// Demo PR !RICH_PR_IDS[1] read from a seeded store at `dir`, its file
    /// list dropped (as before the first sync), over `ado`.
    fn open(
        cx: &mut HeadlessAppContext,
        dir: &std::path::Path,
        ado: Arc<Ado>,
    ) -> anyhow::Result<WindowHandle<PrReview>> {
        let now = adoken_demo::DEMO_NOW;
        let store = adoken_demo::open_store_at(dir, now)?;
        let mut detail = store.pr(RICH_PR_IDS[1])?.context("demo PR in store")?;
        detail.files.clear();
        let people = store.people()?;
        let me = store.me()?.context("demo identity")?.id;
        let blob = |_: &str| None;
        let repo = RepoRef::of(&detail.pr);
        let loaded = data::build(
            data::Source {
                detail,
                people,
                me,
                now,
                viewed: HashSet::new(),
                read_only: false,
                auto_complete: None,
                drafts: Vec::new(),
                blob: &blob,
            },
            0,
        );
        let clock = Arc::new(SystemClock);
        let files = Arc::new(PrFiles::new(
            store.clone(),
            ado,
            Arc::new(Governor::new(clock.clone())),
            clock,
            url::Url::parse("https://dev.azure.com/contoso/")?,
        ));
        let blobs = Blobs::store(&store, Some((files, repo)));
        let window = cx.open_window(size(px(1280.0), px(800.0)), |window, cx| {
            cx.new(|cx| {
                PrReview::new(
                    Some(store),
                    blobs,
                    loaded,
                    Some(Layout::Unified),
                    window,
                    cx,
                )
            })
        })?;
        cx.run_until_parked();
        window.update(cx, |review, window, cx| {
            window.activate_window();
            window.focus(&review.focus_handle(cx), cx);
        })?;
        Ok(window)
    }

    /// Requests finish on Tokio's threads: pump the app until `done`.
    fn wait_for(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<PrReview>,
        what: &str,
        done: impl Fn(&PrReview) -> bool,
    ) -> anyhow::Result<()> {
        for _ in 0..500 {
            cx.run_until_parked();
            if window.update(cx, |review, _, _| done(review))? {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        bail!("timed out waiting for {what}")
    }

    fn dispatch(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<PrReview>,
        action: impl Action,
    ) -> anyhow::Result<()> {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_action(action.boxed_clone(), cx);
        })?;
        cx.run_until_parked();
        Ok(())
    }
}
