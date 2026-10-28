//go:build windows

package app

import (
	"sync"
	"syscall"
	"time"
	"unsafe"

	"github.com/egoist/mygo"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

// dwmwaCloak keeps a shown window out of the desktop image. DWM composites a
// white client if the window is shown before MyGo presents its first frame.
const dwmwaCloak = 13

func prepareStartupWindow(opts *mygo.WindowOptions) {
	// NewWindow shows immediately unless Hidden. Cloak has to be set first.
	opts.Hidden = true
	opts.BackgroundColor = view.ShellBackground
}

func finishStartupWindow(w *mygo.Window) {
	presentStartupWindow(w, func(hwnd uintptr, on bool) {
		if !on {
			// Let DWM take the frame that just presented before the window is visible.
			syscall.NewLazyDLL("dwmapi.dll").NewProc("DwmFlush").Call()
		}
		setWindowCloak(hwnd, on)
	}, func(d time.Duration, fn func()) {
		time.AfterFunc(d, func() { mygo.RunOnMain(fn) })
	})
}

// startupWindow is the part of a window the startup sequence uses.
type startupWindow interface {
	NativeHandle() uintptr
	Show()
	IsDestroyed() bool
	OnReadyToShow(func()) (off func())
}

// presentStartupWindow shows w while cloaked, then removes the cloak after the
// first frame. A missing frame still reveals the window, so startup cannot
// stay invisible. cloak is called on the main thread.
func presentStartupWindow(w startupWindow, cloak func(hwnd uintptr, on bool), after func(time.Duration, func())) {
	var once sync.Once
	shown := false
	show := func() {
		if shown || w.IsDestroyed() {
			return
		}
		shown = true
		w.Show()
	}
	reveal := func() {
		once.Do(func() {
			if w.IsDestroyed() {
				return
			}
			cloak(w.NativeHandle(), false)
			show()
		})
	}
	w.OnReadyToShow(reveal)
	cloak(w.NativeHandle(), true)
	show()
	after(2*time.Second, reveal)
}

func setWindowCloak(hwnd uintptr, on bool) {
	if hwnd == 0 {
		return
	}
	var v int32
	if on {
		v = 1
	}
	syscall.NewLazyDLL("dwmapi.dll").NewProc("DwmSetWindowAttribute").Call(
		hwnd, dwmwaCloak, uintptr(unsafe.Pointer(&v)), unsafe.Sizeof(v))
}
