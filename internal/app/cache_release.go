package app

import (
	"context"
	"fmt"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func (r *Root) requestCacheRelease(path string) {
	if r.busy() {
		return
	}
	for _, category := range r.junk.Categories {
		for _, item := range category.Items {
			if item.Path != path || !core.CacheReleaseEligible(item) {
				continue
			}
			r.start(false, nil, func(context.Context) func() {
				plan, err := core.PrepareCacheRelease(item)
				return func() {
					if err != nil {
						r.failure(err)
						return
					}
					r.confirmCacheRelease(plan, false)
				}
			})
			return
		}
	}
}

func (r *Root) confirmCacheRelease(plan core.CacheReleasePlan, force bool) {
	item := plan.Item()
	detail := item.Path + "\n"
	for _, owner := range plan.Occupants() {
		detail += fmt.Sprintf("\n%s · PID %d\n%s", owner.Name, owner.PID, owner.Executable)
	}
	title := r.tr("退出占用应用并清理缓存？", "Quit cache owners and clean?")
	detail += "\n\n" + r.tr("仅请求已确认的占用进程退出；清理前重新验证身份、占用与扫描计划。", "Only the reviewed owner processes are asked to quit; identities, occupancy and scan plans are revalidated before cleaning.")
	if force {
		title = r.tr("强制结束占用应用并清理缓存？", "Force quit cache owners and clean?")
		detail += "\n\n" + r.tr("强制结束可能丢失未保存的数据。", "Force quitting can lose unsaved data.")
	}
	r.askKind(view.ConfirmReleaseCache, title, detail, func() {
		p := core.NewCleanProgress(item.FileCount, item.Size)
		r.junk.Cleaning = true
		r.start(true, p, func(context.Context) func() {
			outcome, err := core.ExecuteCacheRelease(plan, force, p)
			return func() {
				r.junk.Cleaning = false
				if err != nil {
					r.failure(err)
					return
				}
				if outcome.Cleaned != nil {
					r.finishCleanup("release_cache", []string{item.Path}, *outcome.Cleaned, p)
					if !p.Cancelled() {
						r.scanJunk()
					}
					return
				}
				if outcome.StillBusy && !p.Cancelled() {
					if !force {
						r.confirmCacheRelease(plan, true)
					} else {
						r.message("占用仍未解除；未清理。", "Cache is still occupied; nothing was cleaned.")
					}
				}
			}
		})
	})
	r.confirmation.Force = force
}
