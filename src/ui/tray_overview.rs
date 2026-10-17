//! 空间透镜业务：采样与维护动作；窗口生命周期由 tray_host 提供。
use crate::core::tray_status::{TraySampler, TraySnapshot};
use crate::core::user_accounts::{list_local_accounts, LocalAccount};
use crate::ui::tray_host::{PopoverLifetime, TrayAction, TraySession};
use crate::ui::views::tray_popover::TrayPopover;
use gpui::{App, AppContext, Context, Entity, WeakEntity};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct TrayVolumeInfo {
    pub label: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Clone, Debug)]
pub enum OverviewAction {
    OpenMain,
    ScanJunk,
    OpenView(crate::ui::shell::View),
    AnalyzeDisk(String),
    ReleaseMemory,
    Quit,
}
impl TrayAction for OverviewAction {
    fn dismisses_popover(&self) -> bool {
        !matches!(self, Self::ReleaseMemory)
    }
}

pub struct TrayOverview {
    pub volumes: Vec<TrayVolumeInfo>,
    pub memory_used: u64,
    pub memory_total: u64,
    pub cpu_pct: u32,
    pub temp_c: Option<f32>,
    pub accounts: Vec<LocalAccount>,
    pub purged: bool,
    pub purging: bool,
    pub purge_failed: bool,
    pub swap_used: u64,
    refresh_task: Option<gpui::Task<()>>,
    accounts_task: Option<gpui::Task<()>>,
    lifetime: PopoverLifetime,
}
impl TrayOverview {
    pub fn new(lifetime: PopoverLifetime) -> Self {
        Self {
            volumes: Vec::new(),
            memory_used: 0,
            memory_total: 0,
            cpu_pct: 0,
            temp_c: None,
            accounts: Vec::new(),
            purged: false,
            purging: false,
            purge_failed: false,
            swap_used: 0,
            refresh_task: None,
            accounts_task: None,
            lifetime,
        }
    }
    pub fn start_refresh(&mut self, cx: &mut Context<Self>) {
        if self.refresh_task.is_some() {
            return;
        }
        self.accounts_task = Some(cx.spawn(async move |view, cx| {
            let accounts = cx
                .background_executor()
                .spawn(async { list_local_accounts() })
                .await;
            let _ = view.update(cx, |this, cx| {
                this.accounts = accounts;
                cx.notify();
            });
        }));
        let mut sampler = None;
        self.start_refresh_with(
            move |slow| {
                let sampler = sampler.get_or_insert_with(TraySampler::default);
                sampler.sample(slow)
            },
            cx,
        );
    }

    fn start_refresh_with(
        &mut self,
        mut sample: impl FnMut(bool) -> TraySnapshot + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let lifetime = self.lifetime.clone();
        self.refresh_task = Some(cx.spawn(async move |view, cx| {
            // 初始化和首拍均在后台，CPU 在第一拍只建立基线。
            sample = cx
                .background_executor()
                .spawn(async move {
                    sample(false);
                    sample
                })
                .await;
            cx.background_executor()
                .timer(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL)
                .await;
            let mut tick = 0u64;
            loop {
                // 窗口移除后，GPUI 可能仍持有上一帧的实体，不能只等实体释放。
                if !cx.update(|cx| lifetime.is_open(cx)).unwrap_or(false) {
                    break;
                }
                let (returned, snapshot, volumes) = cx
                    .background_executor()
                    .spawn(async move {
                        let slow = tick.is_multiple_of(5);
                        let snapshot = sample(slow);
                        let volumes = slow.then(|| {
                            crate::platform::list_volumes()
                                .into_iter()
                                .filter_map(|id| {
                                    let (total_bytes, free_bytes) =
                                        crate::platform::get_volume_space(&id)?;
                                    Some(TrayVolumeInfo {
                                        label: id.display().to_string(),
                                        total_bytes,
                                        free_bytes,
                                    })
                                })
                                .collect()
                        });
                        (sample, snapshot, volumes)
                    })
                    .await;
                sample = returned;
                if view
                    .update(cx, |this, cx| {
                        if let Some(volumes) = volumes {
                            this.volumes = volumes;
                        }
                        this.memory_used = snapshot.memory_used;
                        this.memory_total = snapshot.memory_total;
                        this.swap_used = snapshot.swap_used;
                        this.cpu_pct = snapshot.cpu_pct;
                        this.temp_c = snapshot.temp_c;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                tick += 1;
                cx.background_executor().timer(Duration::from_secs(2)).await;
            }
        }));
    }

    pub fn trigger_purge_memory(&mut self, cx: &mut Context<Self>) {
        if self.purging || self.purged {
            return;
        }
        self.purging = true;
        self.purge_failed = false;
        cx.notify();
        cx.spawn(async move |handle, cx| {
            let success = cx
                .background_executor()
                .spawn(async {
                    #[cfg(target_os = "macos")]
                    {
                        std::process::Command::new("/usr/bin/purge")
                            .status()
                            .is_ok_and(|s| s.success())
                    }
                    #[cfg(windows)]
                    {
                        unsafe {
                            use winapi::um::processthreadsapi::GetCurrentProcess;
                            use winapi::um::psapi::EmptyWorkingSet;
                            EmptyWorkingSet(GetCurrentProcess()) != 0
                        }
                    }
                    #[cfg(not(any(target_os = "macos", windows)))]
                    {
                        false
                    }
                })
                .await;
            let _ = handle.update(cx, |this, cx| {
                this.purging = false;
                this.purged = success;
                this.purge_failed = !success;
                cx.notify();
            });
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = handle.update(cx, |this, cx| {
                this.purged = false;
                cx.notify();
            });
        })
        .detach();
    }
}

pub type OverviewDispatcher = Arc<dyn Fn(OverviewAction, &mut App) + Send + Sync>;

/// 仅保存当前模型的弱引用；关窗后不因控制器常驻而保留内容与任务。
#[derive(Clone)]
pub struct OverviewController {
    model: Arc<Mutex<Option<WeakEntity<TrayOverview>>>>,
    dispatcher: OverviewDispatcher,
}
impl OverviewController {
    pub fn new(dispatcher: OverviewDispatcher) -> Self {
        Self {
            model: Arc::new(Mutex::new(None)),
            dispatcher,
        }
    }
    pub fn content(
        &self,
        session: TraySession<OverviewAction>,
        cx: &mut App,
    ) -> Entity<TrayPopover> {
        let model = cx.new(|cx| {
            let mut model = TrayOverview::new(session.lifetime());
            model.start_refresh(cx);
            model
        });
        *self.model.lock().unwrap() = Some(model.downgrade());
        let language = crate::core::settings::Settings::load().language;
        cx.new(|cx| TrayPopover::new(language, model, session, cx))
    }
    pub fn handle(&self, action: OverviewAction, cx: &mut App) {
        match action {
            OverviewAction::ReleaseMemory => {
                let model = self.model.lock().unwrap().clone();
                if let Some(model) = model {
                    let _ = model.update(cx, |model, cx| model.trigger_purge_memory(cx));
                }
            }
            action => (self.dispatcher)(action, cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::i18n::Language;
    use gpui::TestAppContext;
    #[gpui::test]
    fn tray_refresh_updates_repeatedly_and_stops_on_close(cx: &mut TestAppContext) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let host = crate::ui::tray_host::TrayHost::new(360., 560.);
        let mut saved = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            host.mount(
                window,
                cx,
                Arc::new(|_: OverviewAction, _: &mut App| {}),
                |session, _, cx| {
                    let model = cx.new(|cx| {
                        let mut model = TrayOverview::new(session.lifetime());
                        model.start_refresh_with(
                            move |_| {
                                let n = count.fetch_add(1, Ordering::SeqCst) + 1;
                                TraySnapshot {
                                    memory_used: n as u64,
                                    memory_total: 100,
                                    cpu_pct: n as u32,
                                    ..Default::default()
                                }
                            },
                            cx,
                        );
                        model
                    });
                    saved = Some(model.clone());
                    cx.new(|cx| TrayPopover::new(Language::En, model, session, cx))
                },
            )
        });
        let view = saved.unwrap();
        visual.run_until_parked();
        visual
            .executor()
            .advance_clock(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        visual.run_until_parked();
        let first = visual.read(|cx| view.read(cx).memory_used);
        assert!(first >= 2, "baseline is not a displayed sample");
        visual.executor().advance_clock(Duration::from_secs(2));
        visual.run_until_parked();
        assert!(
            visual.read(|cx| view.read(cx).memory_used) > first,
            "second interval must publish a fresh reading"
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        visual.run_until_parked();
        let stopped = calls.load(Ordering::SeqCst);
        visual.executor().advance_clock(Duration::from_secs(20));
        visual.run_until_parked();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            stopped,
            "closed popover must stop sampling"
        );
    }
}
