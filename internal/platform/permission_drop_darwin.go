//go:build darwin

package platform

import (
	"reflect"
	"runtime"
	"sync"
	"sync/atomic"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

// FDA drop panel follows src/platform/macos/permission_drop.rs.
// It is not shown unless System Settings is already visible, or the caller
// explicitly allows the screen-bottom fallback. A bare executable has no
// .app bundle and must not call Show.

const (
	fdaPanelWidth  = 560.0
	fdaPanelHeight = 132.0
	fdaPanelGap    = 12.0
)

type cgPoint struct{ X, Y float64 }
type cgSize struct{ Width, Height float64 }
type cgRect struct {
	Origin cgPoint
	Size   cgSize
}

var (
	fdaPanel     atomic.Uintptr
	fdaArrow     atomic.Uintptr
	fdaQuartz    sync.Once
	fdaQuartzErr error
	copyWindows  func(uint32, uint32) uintptr
)

type FDADropText struct{ AppName, Line1, Line2 string }

func loadFDAFrameworks() error {
	fdaQuartz.Do(func() {
		if _, err := purego.Dlopen("/System/Library/Frameworks/AppKit.framework/AppKit", purego.RTLD_NOW|purego.RTLD_GLOBAL); err != nil {
			fdaQuartzErr = err
			return
		}
		if _, err := purego.Dlopen("/System/Library/Frameworks/QuartzCore.framework/QuartzCore", purego.RTLD_NOW|purego.RTLD_GLOBAL); err != nil {
			fdaQuartzErr = err
			return
		}
		core, err := purego.Dlopen("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics", purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			fdaQuartzErr = err
			return
		}
		purego.RegisterLibFunc(&copyWindows, core, "CGWindowListCopyWindowInfo")
	})
	return fdaQuartzErr
}

func FDADropVisible() bool { return fdaPanel.Load() != 0 }

func HideFDADrop() {
	panel := fdaPanel.Swap(0)
	if panel == 0 {
		return
	}
	fdaArrow.Store(0)
	id := objc.ID(panel)
	id.Send(objc.RegisterName("orderOut:"), uintptr(0))
	id.Send(objc.RegisterName("release"))
}

// ShowFDADrop returns whether the panel is visible. allowFallback is only for
// the timeout after System Settings fails to appear.
func ShowFDADrop(bundle string, text FDADropText, allowFallback bool) bool {
	if bundle == "" || loadFDAFrameworks() != nil {
		return false
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool := objc.ID(objc.GetClass("NSAutoreleasePool")).Send(objc.RegisterName("new"))
	defer pool.Send(objc.RegisterName("drain"))
	rect, found := systemSettingsRect()
	if !found && !allowFallback {
		return false
	}
	panel := objc.ID(fdaPanel.Load())
	if panel == 0 {
		panel = buildFDAPanel(bundle, text)
		if panel == 0 {
			return false
		}
		fdaPanel.Store(uintptr(panel))
	}
	positionFDAPanel(panel, rect, found)
	panel.Send(objc.RegisterName("orderFrontRegardless"))
	return true
}

func buildFDAPanel(bundle string, text FDADropText) objc.ID {
	frame := cgRect{Size: cgSize{fdaPanelWidth, fdaPanelHeight}}
	panel := objc.ID(objc.GetClass("NSPanel")).Send(objc.RegisterName("alloc"))
	panel = objc.Send[objc.ID](panel, objc.RegisterName("initWithContentRect:styleMask:backing:defer:"), frame, uintptr(1<<7), uintptr(2), false)
	if panel == 0 {
		return 0
	}
	panel.Send(objc.RegisterName("setLevel:"), int64(101))
	panel.Send(objc.RegisterName("setCollectionBehavior:"), uintptr(1|1<<4|1<<8))
	panel.Send(objc.RegisterName("setHidesOnDeactivate:"), false)
	panel.Send(objc.RegisterName("setOpaque:"), false)
	panel.Send(objc.RegisterName("setHasShadow:"), true)
	panel.Send(objc.RegisterName("setReleasedWhenClosed:"), false)
	clear := objc.ID(objc.GetClass("NSColor")).Send(objc.RegisterName("clearColor"))
	panel.Send(objc.RegisterName("setBackgroundColor:"), clear)
	panel.Send(objc.RegisterName("setContentView:"), fdaContent(bundle, text))
	return panel
}

func fdaContent(bundle string, text FDADropText) objc.ID {
	frame := cgRect{Size: cgSize{fdaPanelWidth, fdaPanelHeight}}
	container := objc.ID(objc.GetClass("NSView")).Send(objc.RegisterName("alloc"))
	container = objc.Send[objc.ID](container, objc.RegisterName("initWithFrame:"), frame)
	container.Send(objc.RegisterName("setWantsLayer:"), true)
	if layer := container.Send(objc.RegisterName("layer")); layer != 0 {
		layer.Send(objc.RegisterName("setCornerRadius:"), 16.0)
		bg := objc.Send[objc.ID](objc.ID(objc.GetClass("NSColor")), objc.RegisterName("colorWithSRGBRed:green:blue:alpha:"), 0.16, 0.16, 0.17, 0.98)
		layer.Send(objc.RegisterName("setBackgroundColor:"), bg.Send(objc.RegisterName("CGColor")))
	}
	chip := objc.ID(fdaDragClass()).Send(objc.RegisterName("alloc"))
	chip = objc.Send[objc.ID](chip, objc.RegisterName("initWithFrame:"), cgRect{Origin: cgPoint{28, 34}, Size: cgSize{220, 64}})
	setDragPath(chip, bundle)
	chip.Send(objc.RegisterName("setWantsLayer:"), true)
	if layer := chip.Send(objc.RegisterName("layer")); layer != 0 {
		layer.Send(objc.RegisterName("setCornerRadius:"), 12.0)
		bg := objc.Send[objc.ID](objc.ID(objc.GetClass("NSColor")), objc.RegisterName("colorWithSRGBRed:green:blue:alpha:"), 0.26, 0.26, 0.28, 1)
		layer.Send(objc.RegisterName("setBackgroundColor:"), bg.Send(objc.RegisterName("CGColor")))
	}
	icon := workspaceIcon(bundle)
	if icon != 0 {
		view := objc.ID(objc.GetClass("NSImageView")).Send(objc.RegisterName("alloc"))
		view = objc.Send[objc.ID](view, objc.RegisterName("initWithFrame:"), cgRect{Origin: cgPoint{12, 12}, Size: cgSize{40, 40}})
		view.Send(objc.RegisterName("setImage:"), icon)
		chip.Send(objc.RegisterName("addSubview:"), view)
		view.Send(objc.RegisterName("release"))
	}
	name := fdaLabel(cgRect{Origin: cgPoint{62, 20}, Size: cgSize{146, 24}}, text.AppName, 15, true, false)
	chip.Send(objc.RegisterName("addSubview:"), name)
	name.Send(objc.RegisterName("release"))
	container.Send(objc.RegisterName("addSubview:"), chip)
	chip.Send(objc.RegisterName("release"))
	arrow := fdaLabel(cgRect{Origin: cgPoint{268, 46}, Size: cgSize{40, 40}}, "↑", 30, false, true)
	container.Send(objc.RegisterName("addSubview:"), arrow)
	fdaArrow.Store(uintptr(arrow))
	arrow.Send(objc.RegisterName("release"))
	line1 := fdaLabel(cgRect{Origin: cgPoint{318, 62}, Size: cgSize{220, 22}}, text.Line1, 13, true, false)
	line2 := fdaLabel(cgRect{Origin: cgPoint{318, 40}, Size: cgSize{220, 22}}, text.Line2, 13, false, false)
	container.Send(objc.RegisterName("addSubview:"), line1)
	container.Send(objc.RegisterName("addSubview:"), line2)
	line1.Send(objc.RegisterName("release"))
	line2.Send(objc.RegisterName("release"))
	return container
}

func fdaLabel(frame cgRect, text string, size float64, bold, center bool) objc.ID {
	field := objc.ID(objc.GetClass("NSTextField")).Send(objc.RegisterName("alloc"))
	field = objc.Send[objc.ID](field, objc.RegisterName("initWithFrame:"), frame)
	ns := nsString(text)
	field.Send(objc.RegisterName("setStringValue:"), ns)
	ns.Send(objc.RegisterName("release"))
	field.Send(objc.RegisterName("setEditable:"), false)
	field.Send(objc.RegisterName("setSelectable:"), false)
	field.Send(objc.RegisterName("setBezeled:"), false)
	field.Send(objc.RegisterName("setDrawsBackground:"), false)
	var font objc.ID
	if bold {
		font = objc.Send[objc.ID](objc.ID(objc.GetClass("NSFont")), objc.RegisterName("boldSystemFontOfSize:"), size)
	} else {
		font = objc.Send[objc.ID](objc.ID(objc.GetClass("NSFont")), objc.RegisterName("systemFontOfSize:"), size)
	}
	field.Send(objc.RegisterName("setFont:"), font)
	color := objc.Send[objc.ID](objc.ID(objc.GetClass("NSColor")), objc.RegisterName("colorWithSRGBRed:green:blue:alpha:"), 0.78, 0.78, 0.80, 1.0)
	if bold {
		color = objc.Send[objc.ID](objc.ID(objc.GetClass("NSColor")), objc.RegisterName("colorWithSRGBRed:green:blue:alpha:"), 1.0, 1.0, 1.0, 1.0)
	}
	field.Send(objc.RegisterName("setTextColor:"), color)
	if center {
		field.Send(objc.RegisterName("setAlignment:"), uintptr(1))
	}
	return field
}

func nsString(s string) objc.ID {
	obj := objc.ID(objc.GetClass("NSString")).Send(objc.RegisterName("alloc"))
	return objc.Send[objc.ID](obj, objc.RegisterName("initWithBytes:length:encoding:"), unsafe.StringData(s), len(s), uintptr(4))
}

func workspaceIcon(path string) objc.ID {
	ws := objc.ID(objc.GetClass("NSWorkspace")).Send(objc.RegisterName("sharedWorkspace"))
	ns := nsString(path)
	defer ns.Send(objc.RegisterName("release"))
	return ws.Send(objc.RegisterName("iconForFile:"), ns)
}

func positionFDAPanel(panel objc.ID, settings cgRect, found bool) {
	var x, y float64
	if found {
		screens := objc.ID(objc.GetClass("NSScreen")).Send(objc.RegisterName("screens"))
		mainH := objc.Send[cgRect](screens.Send(objc.RegisterName("objectAtIndex:"), 0), objc.RegisterName("frame")).Size.Height
		bottom := mainH - (settings.Origin.Y + settings.Size.Height)
		x = settings.Origin.X + (settings.Size.Width-fdaPanelWidth)/2
		y = bottom - fdaPanelGap - fdaPanelHeight
	} else {
		visible := objc.Send[cgRect](objc.ID(objc.GetClass("NSScreen")).Send(objc.RegisterName("mainScreen")), objc.RegisterName("visibleFrame"))
		x = visible.Origin.X + (visible.Size.Width-fdaPanelWidth)/2
		y = visible.Origin.Y + 24
	}
	visible := objc.Send[cgRect](objc.ID(objc.GetClass("NSScreen")).Send(objc.RegisterName("mainScreen")), objc.RegisterName("visibleFrame"))
	x = max(visible.Origin.X+8, min(x, visible.Origin.X+visible.Size.Width-fdaPanelWidth-8))
	y = max(visible.Origin.Y+8, min(y, visible.Origin.Y+visible.Size.Height-fdaPanelHeight-8))
	panel.Send(objc.RegisterName("setFrameOrigin:"), cgPoint{x, y})
}

func systemSettingsRect() (cgRect, bool) {
	bid := nsString("com.apple.systempreferences")
	defer bid.Send(objc.RegisterName("release"))
	apps := objc.ID(objc.GetClass("NSRunningApplication")).Send(objc.RegisterName("runningApplicationsWithBundleIdentifier:"), bid)
	if apps == 0 || apps.Send(objc.RegisterName("count")) == 0 {
		return cgRect{}, false
	}
	app := apps.Send(objc.RegisterName("objectAtIndex:"), 0)
	pid := objc.Send[int32](app, objc.RegisterName("processIdentifier"))
	if copyWindows == nil {
		return cgRect{}, false
	}
	list := objc.ID(copyWindows(1, 0))
	if list == 0 {
		return cgRect{}, false
	}
	defer list.Send(objc.RegisterName("release"))
	keyPID, keyBounds := nsString("kCGWindowOwnerPID"), nsString("kCGWindowBounds")
	defer keyPID.Send(objc.RegisterName("release"))
	defer keyBounds.Send(objc.RegisterName("release"))
	var best cgRect
	var ok bool
	count := int(list.Send(objc.RegisterName("count")))
	for i := range count {
		info := list.Send(objc.RegisterName("objectAtIndex:"), i)
		owner := info.Send(objc.RegisterName("objectForKey:"), keyPID)
		if owner == 0 || objc.Send[int32](owner, objc.RegisterName("intValue")) != pid {
			continue
		}
		dict := info.Send(objc.RegisterName("objectForKey:"), keyBounds)
		rect, found := rectFromDict(dict)
		if !found || rect.Size.Width < 400 || rect.Size.Height < 300 {
			continue
		}
		if !ok || rect.Size.Width*rect.Size.Height > best.Size.Width*best.Size.Height {
			best, ok = rect, true
		}
	}
	return best, ok
}

func rectFromDict(dict objc.ID) (cgRect, bool) {
	if dict == 0 {
		return cgRect{}, false
	}
	vals := [4]float64{}
	for i, key := range []string{"X", "Y", "Width", "Height"} {
		ns := nsString(key)
		number := dict.Send(objc.RegisterName("objectForKey:"), ns)
		ns.Send(objc.RegisterName("release"))
		if number == 0 {
			return cgRect{}, false
		}
		vals[i] = objc.Send[float64](number, objc.RegisterName("doubleValue"))
	}
	return cgRect{Origin: cgPoint{vals[0], vals[1]}, Size: cgSize{vals[2], vals[3]}}, true
}

var fdaDragOnce sync.Once
var fdaDrag objc.Class

func fdaDragClass() objc.Class {
	fdaDragOnce.Do(func() {
		class, err := objc.RegisterClass("QCPermissionDragSourceView", objc.GetClass("NSView"), nil, []objc.FieldDef{{Name: "qcDragPath", Type: reflect.TypeFor[uintptr](), Attribute: objc.ReadWrite}}, []objc.MethodDef{
			{Cmd: objc.RegisterName("acceptsFirstMouse:"), Fn: fdaAcceptsFirstMouse},
			{Cmd: objc.RegisterName("mouseDown:"), Fn: fdaMouseDown},
			{Cmd: objc.RegisterName("draggingSession:sourceOperationMaskForDraggingContext:"), Fn: fdaDragMask},
		})
		if err == nil {
			fdaDrag = class
		}
	})
	if fdaDrag == 0 {
		return objc.GetClass("NSView")
	}
	return fdaDrag
}

func setDragPath(view objc.ID, path string) {
	ns := nsString(path)
	ivar := fdaDragClass().InstanceVariable("qcDragPath")
	view.SetIvar(ivar, ns)
}

func fdaAcceptsFirstMouse(objc.ID, objc.SEL, objc.ID) bool { return true }

func fdaDragMask(objc.ID, objc.SEL, objc.ID, int) uintptr { return 1 }

func fdaMouseDown(self objc.ID, _ objc.SEL, event objc.ID) {
	pool := objc.ID(objc.GetClass("NSAutoreleasePool")).Send(objc.RegisterName("new"))
	defer pool.Send(objc.RegisterName("drain"))
	path := self.GetIvar(fdaDragClass().InstanceVariable("qcDragPath"))
	if path == 0 {
		return
	}
	url := objc.ID(objc.GetClass("NSURL")).Send(objc.RegisterName("fileURLWithPath:"), path)
	item := objc.ID(objc.GetClass("NSDraggingItem")).Send(objc.RegisterName("alloc"))
	item = item.Send(objc.RegisterName("initWithPasteboardWriter:"), url)
	if item == 0 {
		return
	}
	defer item.Send(objc.RegisterName("release"))
	icon := objc.ID(objc.GetClass("NSWorkspace")).Send(objc.RegisterName("sharedWorkspace")).Send(objc.RegisterName("iconForFile:"), path)
	if icon != 0 {
		icon.Send(objc.RegisterName("setSize:"), cgSize{48, 48})
		item.Send(objc.RegisterName("setDraggingFrame:contents:"), cgRect{Origin: cgPoint{12, 8}, Size: cgSize{48, 48}}, icon)
	}
	items := objc.ID(objc.GetClass("NSArray")).Send(objc.RegisterName("arrayWithObject:"), item)
	self.Send(objc.RegisterName("beginDraggingSessionWithItems:event:source:"), items, event, self)
}
