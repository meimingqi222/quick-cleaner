package platform

import (
	"fmt"
	"os"
	"strings"
	"testing"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

var getHandleInformation = windows.NewLazySystemDLL("kernel32.dll").NewProc("GetHandleInformation")

func isolatedInstance(t *testing.T) {
	t.Helper()
	oldMutex, oldWake := instanceMutexName, instanceWakeName
	suffix := fmt.Sprintf("%d.%d", os.Getpid(), time.Now().UnixNano())
	instanceMutexName = `Local\QuickCleaner.Test.Instance.` + suffix
	instanceWakeName = `Local\QuickCleaner.Test.Wake.` + suffix
	t.Cleanup(func() {
		if instance.wake != 0 {
			windows.CloseHandle(instance.wake)
		}
		if instance.mutex != 0 {
			windows.CloseHandle(instance.mutex)
		}
		instance.mutex, instance.wake = 0, 0
		instanceMutexName, instanceWakeName = oldMutex, oldWake
	})
}

func TestSecondLaunchFindsAndWakesTheInstance(t *testing.T) {
	isolatedInstance(t)
	if InstanceRunning() {
		t.Fatal("a fresh name reports an instance")
	}
	if !AcquireInstance() {
		t.Fatal("the first launch did not become the instance")
	}
	if !InstanceRunning() {
		t.Fatal("a second launch cannot see the instance")
	}
	held := instance.mutex
	if AcquireInstance() || instance.mutex != held {
		t.Fatal("a second launch took the instance over")
	}
	woken := make(chan struct{}, 1)
	OnInstanceWake(func() { woken <- struct{}{} })
	if !WakeRunningInstance() {
		t.Fatal("the wake signal was not delivered")
	}
	select {
	case <-woken:
	case <-time.After(2 * time.Second):
		t.Fatal("the instance was not woken")
	}
}

// A plain launch must reach an elevated instance: both objects carry a
// Medium label, and the mutex must not be inherited by children that may
// outlive the app (the update helper).
func TestInstanceObjectsAreReachableFromMediumIntegrity(t *testing.T) {
	isolatedInstance(t)
	if !AcquireInstance() {
		t.Fatal("acquire")
	}
	for name, h := range map[string]windows.Handle{"mutex": instance.mutex, "wake": instance.wake} {
		sd, err := windows.GetSecurityInfo(h, windows.SE_KERNEL_OBJECT, windows.LABEL_SECURITY_INFORMATION)
		if err != nil {
			t.Fatal(name, err)
		}
		if label := sd.String(); !strings.Contains(label, ";;;ME)") {
			t.Fatalf("%s label %q is not Medium", name, label)
		}
		var flags uint32
		if r, _, err := getHandleInformation.Call(uintptr(h), uintptr(unsafe.Pointer(&flags))); r == 0 {
			t.Fatal(name, err)
		}
		if flags&windows.HANDLE_FLAG_INHERIT != 0 {
			t.Fatalf("%s handle is inheritable", name)
		}
	}
}
