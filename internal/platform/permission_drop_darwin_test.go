//go:build darwin

package platform

import (
	"fmt"
	"math"
	"os"
	"os/exec"
	"runtime"
	"testing"

	"github.com/ebitengine/purego/objc"
)

// AppKit image preparation requires the process main thread. Run the native
// checks during child initialization, before testing moves work to goroutines.
func init() {
	if os.Getenv("QC_TEST_FDA_BOUNCE") != "1" {
		return
	}
	runtime.LockOSThread()
	checkFDADropArrowBounce(func(args ...any) {
		fmt.Fprintln(os.Stderr, args...)
		os.Exit(1)
	})
	os.Exit(0)
}

func TestFDADropArrowHasRustBounce(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	cmd := exec.Command(executable, "-test.run=^TestFDADropArrowHasRustBounce$")
	cmd.Env = append(os.Environ(), "QC_TEST_FDA_BOUNCE=1")
	if output, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("native FDA bounce: %v\n%s", err, output)
	}
}

func checkFDADropArrowBounce(fatal func(...any)) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	if err := loadFDAFrameworks(); err != nil {
		fatal(err)
	}
	pool := objc.ID(objc.GetClass("NSAutoreleasePool")).Send(objc.RegisterName("new"))
	defer pool.Send(objc.RegisterName("drain"))
	previous := fdaArrow.Load()
	defer fdaArrow.Store(previous)
	content := fdaContent("/Applications/QuickCleaner.app", FDADropText{AppName: "QuickCleaner"})
	defer content.Send(objc.RegisterName("release"))
	arrow := objc.ID(fdaArrow.Load())
	layer := arrow.Send(objc.RegisterName("layer"))
	key := nsString("bounce")
	defer key.Send(objc.RegisterName("release"))
	animation := layer.Send(objc.RegisterName("animationForKey:"), key)
	if animation == 0 {
		fatal("FDA arrow is static: missing Rust bounce animation")
	}
	from := objc.Send[float64](animation.Send(objc.RegisterName("fromValue")), objc.RegisterName("doubleValue"))
	to := objc.Send[float64](animation.Send(objc.RegisterName("toValue")), objc.RegisterName("doubleValue"))
	if to-from != 8 || objc.Send[float64](animation, objc.RegisterName("duration")) != 0.6 || !objc.Send[bool](animation, objc.RegisterName("autoreverses")) || !math.IsInf(float64(objc.Send[float32](animation, objc.RegisterName("repeatCount"))), 1) {
		fatal("FDA bounce must match Rust: +8pt, 0.6s, autoreverse, infinite repeat")
	}
	path := nsString("position.y")
	defer path.Send(objc.RegisterName("release"))
	if !objc.Send[bool](animation.Send(objc.RegisterName("keyPath")), objc.RegisterName("isEqualToString:"), path) {
		fatal("FDA arrow must bounce vertically")
	}
	// Core Animation stores immutable copies; a sentinel start time detects
	// replacement without relying on object identity or wall-clock sleeps.
	animation = animation.Send(objc.RegisterName("copy"))
	defer animation.Send(objc.RegisterName("release"))
	animation.Send(objc.RegisterName("setBeginTime:"), 123.0)
	layer.Send(objc.RegisterName("addAnimation:forKey:"), animation, key)
	for range 5 {
		ensureFDABounceAnimation()
	}
	current := layer.Send(objc.RegisterName("animationForKey:"), key)
	if objc.Send[float64](current, objc.RegisterName("beginTime")) != 123 {
		fatal("repeated display restarted the running bounce")
	}
	layer.Send(objc.RegisterName("removeAnimationForKey:"), key)
	ensureFDABounceAnimation()
	if layer.Send(objc.RegisterName("animationForKey:"), key) == 0 {
		fatal("display must restore a detached bounce")
	}
}
