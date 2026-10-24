package platform

import (
	"errors"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

// The instance objects live in the logon session's Local\ namespace: two
// users on one PC each get their own instance, while the desktop user's
// launch and an elevated copy (even under another admin account) share one.
var (
	instanceMutexName = `Local\QuickCleaner.Instance`
	instanceWakeName  = `Local\QuickCleaner.Wake`
)

// instanceSDDL lets a medium-integrity launch see and wake an elevated
// instance: authenticated users may synchronize and signal, and the
// mandatory label is lowered to Medium. Objects an elevated process creates
// are otherwise High, and a plain launch could neither detect nor wake it,
// starting a second instance or a needless UAC prompt instead.
const instanceSDDL = "D:(A;;GA;;;OW)(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x00100002;;;AU)S:(ML;;NW;;;ME)"

var instance struct {
	mutex, wake windows.Handle
}

var allowSetForeground = windows.NewLazySystemDLL("user32.dll").NewProc("AllowSetForegroundWindow")

func instanceAttributes() (*windows.SecurityAttributes, error) {
	sd, err := windows.SecurityDescriptorFromString(instanceSDDL)
	if err != nil {
		return nil, err
	}
	// Not inheritable: a child that outlived us (the update helper) would
	// keep the mutex alive and make the next launch think we still run.
	return &windows.SecurityAttributes{Length: uint32(unsafe.Sizeof(windows.SecurityAttributes{})), SecurityDescriptor: sd, InheritHandle: 0}, nil
}

// InstanceRunning reports whether another QuickCleaner owns the session's
// instance mutex. Access denied still proves it exists.
func InstanceRunning() bool {
	name, err := windows.UTF16PtrFromString(instanceMutexName)
	if err != nil {
		return false
	}
	h, err := windows.OpenMutex(windows.SYNCHRONIZE, false, name)
	if err == nil {
		windows.CloseHandle(h)
		return true
	}
	return errors.Is(err, windows.ERROR_ACCESS_DENIED)
}

// AcquireInstance makes this process the session's instance and creates its
// wake event, so a launch that arrives before the window exists is queued.
// It returns false when another instance already owns the mutex. When the
// objects cannot be created at all, it behaves as the only instance rather
// than refusing to start.
func AcquireInstance() bool {
	sa, err := instanceAttributes()
	if err != nil {
		return true
	}
	name, err := windows.UTF16PtrFromString(instanceMutexName)
	if err != nil {
		return true
	}
	h, err := windows.CreateMutex(sa, false, name)
	switch {
	case errors.Is(err, windows.ERROR_ALREADY_EXISTS):
		windows.CloseHandle(h)
		return false
	case errors.Is(err, windows.ERROR_ACCESS_DENIED):
		return false
	case err != nil:
		return true
	}
	instance.mutex = h
	if wake, err := windows.UTF16PtrFromString(instanceWakeName); err == nil {
		// Auto-reset: one launch, one wake-up.
		if e, err := windows.CreateEvent(sa, 0, 0, wake); err == nil {
			instance.wake = e
		}
	}
	return true
}

// WakeRunningInstance asks the running instance to show its window. The
// launch that the user just started holds the foreground right and hands it
// over, so the woken window may come to the front. The instance may still
// be starting, so the wake event is retried briefly.
func WakeRunningInstance() bool {
	name, err := windows.UTF16PtrFromString(instanceWakeName)
	if err != nil {
		return false
	}
	const asfwAny = ^uintptr(0)
	allowSetForeground.Call(asfwAny)
	for range 30 {
		if h, err := windows.OpenEvent(windows.EVENT_MODIFY_STATE, false, name); err == nil {
			err = windows.SetEvent(h)
			windows.CloseHandle(h)
			return err == nil
		}
		time.Sleep(100 * time.Millisecond)
	}
	return false
}

// OnInstanceWake calls fn on its own goroutine each time a later launch
// wakes this instance. It does nothing unless AcquireInstance succeeded.
func OnInstanceWake(fn func()) {
	if instance.wake == 0 {
		return
	}
	go func() {
		for {
			if ev, err := windows.WaitForSingleObject(instance.wake, windows.INFINITE); err != nil || ev != windows.WAIT_OBJECT_0 {
				return
			}
			fn()
		}
	}()
}
