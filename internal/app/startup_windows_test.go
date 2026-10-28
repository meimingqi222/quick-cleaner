//go:build windows

package app

import (
	"testing"
	"time"

	"github.com/egoist/mygo"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

type fakeStartupWindow struct {
	hwnd      uintptr
	destroyed bool
	ready     func()
	shows     int
}

func (f *fakeStartupWindow) NativeHandle() uintptr { return f.hwnd }
func (f *fakeStartupWindow) Show() {
	f.shows++
}
func (f *fakeStartupWindow) IsDestroyed() bool { return f.destroyed }
func (f *fakeStartupWindow) OnReadyToShow(fn func()) func() {
	f.ready = fn
	return func() {}
}

func TestStartupWindowStaysCloakedUntilFirstFrame(t *testing.T) {
	var opts mygo.WindowOptions
	prepareStartupWindow(&opts)
	if !opts.Hidden {
		t.Fatal("startup window is shown before it can be cloaked")
	}
	if opts.BackgroundColor != view.ShellBackground {
		t.Fatalf("background %q", opts.BackgroundColor)
	}

	w := &fakeStartupWindow{hwnd: 42}
	var cloaks []bool
	var fallback time.Duration
	var delayed func()
	presentStartupWindow(w, func(hwnd uintptr, on bool) {
		if hwnd != w.hwnd {
			t.Fatalf("cloak hwnd %d", hwnd)
		}
		cloaks = append(cloaks, on)
	}, func(d time.Duration, fn func()) {
		fallback = d
		delayed = fn
	})
	if w.ready == nil {
		t.Fatal("first frame has no reveal")
	}
	if fallback < time.Second {
		t.Fatalf("fallback %s reveals the window before a frame", fallback)
	}
	if len(cloaks) != 1 || !cloaks[0] || w.shows != 1 {
		t.Fatalf("before frame cloaks=%v shows=%d", cloaks, w.shows)
	}

	w.ready()
	if len(cloaks) != 2 || cloaks[1] || w.shows != 1 {
		t.Fatalf("after frame cloaks=%v shows=%d", cloaks, w.shows)
	}
	delayed()
	if len(cloaks) != 2 || w.shows != 1 {
		t.Fatalf("fallback repeated reveal cloaks=%v shows=%d", cloaks, w.shows)
	}
}

func TestStartupWindowRevealsWhenFrameNeverArrives(t *testing.T) {
	w := &fakeStartupWindow{hwnd: 7}
	var cloaks []bool
	var delayed func()
	presentStartupWindow(w, func(_ uintptr, on bool) {
		cloaks = append(cloaks, on)
	}, func(_ time.Duration, fn func()) { delayed = fn })
	delayed()
	if len(cloaks) != 2 || cloaks[1] || w.shows != 1 {
		t.Fatalf("fallback cloaks=%v shows=%d", cloaks, w.shows)
	}
}

func TestStartupWindowDestroyedBeforeFrameStaysHidden(t *testing.T) {
	w := &fakeStartupWindow{hwnd: 7}
	var cloaks []bool
	presentStartupWindow(w, func(_ uintptr, on bool) {
		cloaks = append(cloaks, on)
	}, func(time.Duration, func()) {})
	w.destroyed = true
	w.ready()
	if len(cloaks) != 1 || !cloaks[0] || w.shows != 1 {
		t.Fatalf("destroyed reveal cloaks=%v shows=%d", cloaks, w.shows)
	}
}
