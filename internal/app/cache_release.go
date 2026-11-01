package app

import (
	"context"
	"fmt"
	"strings"

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
						r.cacheReleaseFailure(err)
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
					r.cacheReleaseFailure(err)
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

// cacheReleaseFailure keeps system/other-user refusals out of the raw-error
// channel: those are expected skip states, not failures the user can fix.
func (r *Root) cacheReleaseFailure(err error) {
	text := strings.ToLower(err.Error())
	switch {
	case strings.Contains(text, "system-or-other-user-process"),
		strings.Contains(text, "service-or-critical-owner"),
		strings.Contains(text, "critical-owner"),
		strings.Contains(text, "system-process"),
		strings.Contains(text, "background-service"),
		strings.Contains(text, "owner-executable-changed-or-protected"),
		strings.Contains(text, "cleaner-process-tree"):
		r.message("系统、其他用户或受保护进程占用，无法结束；已跳过。", "A system, another user's, or a protected process holds the cache; skipped.")
	case strings.Contains(text, "no-confirmed-cache-owners"):
		r.message("没有确认到占用进程；请重新扫描后再试。", "No confirmed cache owner; rescan and try again.")
	case strings.Contains(text, "owners-changed"), strings.Contains(text, "owner-identity-changed"),
		strings.Contains(text, "owner-executable-changed"), strings.Contains(text, "owner-exited"):
		r.message("占用进程已变化；请重新扫描后再试。", "Cache owners changed; rescan and try again.")
	default:
		r.failure(err)
	}
}
