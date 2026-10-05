use gpui::Context;
use std::time::Duration;

impl crate::ui::Root {
    pub fn start_rule_scheduler(&mut self, cx: &mut Context<Self>) {
        if self.rule_update.task.is_some() || !crate::platform::is_packaged_install() {
            return;
        }
        self.rule_update.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(15))
                .await;
            loop {
                let keep = this
                    .update(cx, |this, cx| {
                        if this.settings.auto_update_rules {
                            this.check_rule_update(cx);
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    return;
                }
                cx.background_executor()
                    .timer(Duration::from_secs(
                        crate::core::rules::update::CHECK_INTERVAL,
                    ))
                    .await;
            }
        }));
    }
    pub fn check_rule_update(&mut self, cx: &mut Context<Self>) {
        if self.rule_update.checking {
            return;
        }
        self.rule_update.checking = true;
        self.rule_update.error = None;
        cx.notify();
        let work = cx
            .background_executor()
            .spawn(async { crate::core::rules::update::check_and_update() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                this.rule_update.checking = false;
                this.rule_update.error = result.err();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub fn rollback_rules(&mut self, cx: &mut Context<Self>) {
        if self.rule_update.checking {
            return;
        }
        self.rule_update.checking = true;
        let work = cx
            .background_executor()
            .spawn(async { crate::core::rules::update::rollback() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                this.rule_update.checking = false;
                this.rule_update.error = result.err();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
