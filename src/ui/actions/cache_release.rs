use crate::core::{
    cache_release::{self, ReleaseOutcome, ReleasePlan},
    cleaner::{CleanProgress, CleanReport},
    i18n::bilingual,
    model::fmt_size,
};
use crate::ui::{
    adapters::{ConfirmKind, ConfirmRequest},
    i18n::*,
    Root,
};
use gpui::Context;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

enum ReleaseResult {
    Cleaned,
    StillBusy,
    Refused(String),
}

impl Root {
    pub fn request_release_cache(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.clean.running || self.junk.scanning {
            return;
        }
        let Some(item) = self.junk.items().find(|item| item.path == path).cloned() else {
            return;
        };
        if !cache_release::eligible(&item) {
            return;
        }
        let generation = self.junk.gen;
        self.clean.running = true;
        self.clean.progress = Some(Arc::new(CleanProgress::default()));
        self.status = bilingual(|lang| tr_release_cache_status(lang, "probe", ""));
        self.start_tick(cx);
        let work = cx
            .background_executor()
            .spawn(async move { cache_release::prepare(item) });
        self.clean.task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                this.clean.running = false;
                if this.junk.gen != generation
                    || this.clean.progress.as_ref().is_some_and(|p| p.cancelled())
                {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(plan) => this.show_release_cache_confirm(plan, false, cx),
                    Err(error) => {
                        this.status =
                            bilingual(|lang| tr_release_cache_status(lang, "error", &error));
                        cx.notify();
                    }
                }
            })
            .ok();
        }));
        cx.notify();
    }

    fn show_release_cache_confirm(
        &mut self,
        plan: ReleasePlan,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        self.confirm = Some(ConfirmRequest {
            title: tr_release_cache(self.language, force).into(),
            body: plan.item.label.get(self.language).into(),
            detail: tr_release_cache_detail(self.language, &plan, force),
            kind: ConfirmKind::ReleaseCache {
                plan: Box::new(plan),
                force,
            },
            app_data: false,
        });
        cx.notify();
    }

    pub fn execute_release_cache(
        &mut self,
        plan: ReleasePlan,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        if self.clean.running {
            return;
        }
        let outcome = Arc::new(Mutex::new(None));
        let work_outcome = outcome.clone();
        let work_plan = plan.clone();
        let generation = self.junk.gen;
        self.spawn_clean(
            (plan.item.file_count, plan.item.size),
            bilingual(|lang| tr_release_cache_status(lang, "wait", "")),
            move |progress| {
                let result = cache_release::execute(&work_plan, force, progress);
                match result {
                    Ok(ReleaseOutcome::Cleaned(report)) => {
                        *work_outcome.lock().unwrap() = Some(ReleaseResult::Cleaned);
                        report
                    }
                    Ok(ReleaseOutcome::StillBusy) => {
                        *work_outcome.lock().unwrap() = Some(ReleaseResult::StillBusy);
                        CleanReport::default()
                    }
                    Err(error) => {
                        *work_outcome.lock().unwrap() = Some(ReleaseResult::Refused(error));
                        CleanReport::default()
                    }
                }
            },
            move |this, report, snapshot, cx| {
                let result = outcome.lock().unwrap().take();
                if this.junk.gen != generation {
                    return;
                }
                match result {
                    Some(ReleaseResult::StillBusy) if !force && !snapshot.cancelled => {
                        this.show_release_cache_confirm(plan, true, cx)
                    }
                    Some(ReleaseResult::StillBusy) => {
                        this.status = bilingual(|lang| tr_release_cache_status(lang, "busy", ""))
                    }
                    Some(ReleaseResult::Refused(error)) => {
                        this.status =
                            bilingual(|lang| tr_release_cache_status(lang, "error", &error))
                    }
                    Some(ReleaseResult::Cleaned) => {
                        let unresolved = snapshot.cancelled
                            || !report.failed.is_empty()
                            || report.skipped > 0
                            || !report.manual.is_empty();
                        let path = plan.item.path;
                        this.apply_clean_result(
                            std::slice::from_ref(&path),
                            if unresolved {
                                std::slice::from_ref(&path)
                            } else {
                                &[]
                            },
                        );
                        this.junk.apply_remaining_sizes(&report.remaining_live);
                        if let Some(item) = this
                            .junk
                            .categories
                            .iter_mut()
                            .flat_map(|cat| &mut cat.items)
                            .find(|item| item.path == path)
                        {
                            if !unresolved {
                                item.busy = None;
                            }
                        }
                        this.clean.last_failed.retain(|item| item.path != path);
                        this.clean
                            .last_failed
                            .extend(report.fail_info.iter().map(|info| {
                                let (bytes, files) = report
                                    .remaining_live
                                    .iter()
                                    .find(|(path, _, _)| *path == info.path)
                                    .map(|(_, bytes, files)| (*bytes, *files))
                                    .unwrap_or((0, 0));
                                crate::ui::state::FailedItem {
                                    path: info.path.clone(),
                                    bytes,
                                    files,
                                    reason: info.reason,
                                    failed_leaves: info.failed_leaves,
                                }
                            }));
                        this.clean.last_failed_files = this
                            .clean
                            .last_failed
                            .iter()
                            .map(|item| item.failed_leaves as u64)
                            .sum();
                        this.disk.mft = None;
                        #[cfg(not(windows))]
                        {
                            this.macos_root_index = None;
                        }
                        this.status = bilingual(|lang| {
                            tr_release_cache_status(
                                lang,
                                if unresolved { "error" } else { "done" },
                                &fmt_size(snapshot.bytes),
                            )
                        });
                    }
                    _ => {}
                }
                cx.notify();
            },
            cx,
        );
    }
}
