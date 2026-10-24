package platform

import (
	"image"
	"os"
	"runtime"
	"strings"
	"sync"
	"unsafe"

	"golang.org/x/sys/windows"
)

var iconGDILock sync.Mutex
var iconUser = windows.NewLazySystemDLL("user32.dll")
var iconGDI = windows.NewLazySystemDLL("gdi32.dll")
var iconShell = windows.NewLazySystemDLL("shell32.dll")

// AppIconPNG extracts an image or native module icon; nil means unavailable.
func AppIconPNG(path string) []byte { return AppIconFromBundle(path) }

// AppIconFromBundle also accepts installation directories and DisplayIcon hints.
func AppIconFromBundle(path string) []byte {
	path = iconDisplayPath(path)
	if strings.Contains(path, "%") {
		if p, e := windows.UTF16PtrFromString(path); e == nil {
			var buf [1024]uint16
			n, _, _ := windows.NewLazySystemDLL("kernel32.dll").NewProc("ExpandEnvironmentStringsW").Call(uintptr(unsafe.Pointer(p)), uintptr(unsafe.Pointer(&buf[0])), 1024)
			if n > 0 && n <= 1024 {
				path = windows.UTF16ToString(buf[:n])
			}
		}
	}
	if s := strings.TrimRight(path, "\\/"); s != "" {
		path = s
	}
	if iconRegular(path) {
		return iconWindowsFile(path)
	}
	if s, e := os.Stat(path); e == nil && s.IsDir() {
		for _, p := range iconWindowsCandidates(path) {
			if b := iconWindowsFile(p); b != nil {
				return b
			}
		}
	}
	return nil
}

func iconWindowsFile(path string) []byte {
	if b := iconWindowsImage(path); b != nil {
		return b
	}
	p, e := windows.UTF16PtrFromString(path)
	if e != nil {
		return nil
	}
	iconGDILock.Lock()
	defer iconGDILock.Unlock()
	destroy := iconUser.NewProc("DestroyIcon")
	var h uintptr
	var id uint32
	proc := iconUser.NewProc("PrivateExtractIconsW")
	if proc.Find() == nil {
		n, _, _ := proc.Call(uintptr(unsafe.Pointer(p)), 0, 48, 48, uintptr(unsafe.Pointer(&h)), uintptr(unsafe.Pointer(&id)), 1, 0)
		if n == 0 || n == 0xffffffff {
			if h != 0 {
				destroy.Call(h)
				h = 0
			}
		}
	}
	if h == 0 {
		var large, small uintptr
		n, _, _ := iconShell.NewProc("ExtractIconExW").Call(uintptr(unsafe.Pointer(p)), 0, uintptr(unsafe.Pointer(&large)), uintptr(unsafe.Pointer(&small)), 1)
		if small != 0 {
			destroy.Call(small)
		}
		if n > 0 {
			h = large
		} else if large != 0 {
			destroy.Call(large)
		}
	}
	if h == 0 {
		var info struct {
			Icon       uintptr
			Index      int32
			Attributes uint32
			Name       [260]uint16
			Type       [80]uint16
		}
		n, _, _ := iconShell.NewProc("SHGetFileInfoW").Call(uintptr(unsafe.Pointer(p)), 0, uintptr(unsafe.Pointer(&info)), unsafe.Sizeof(info), 0x100)
		if n != 0 {
			h = info.Icon
		} else if info.Icon != 0 {
			destroy.Call(info.Icon)
		}
	}
	runtime.KeepAlive(p)
	if h == 0 {
		return nil
	}
	defer destroy.Call(h)
	screen, _, _ := iconUser.NewProc("GetDC").Call(0)
	if screen == 0 {
		return nil
	}
	dc, _, _ := iconGDI.NewProc("CreateCompatibleDC").Call(screen)
	iconUser.NewProc("ReleaseDC").Call(0, screen)
	if dc == 0 {
		return nil
	}
	defer iconGDI.NewProc("DeleteDC").Call(dc)
	var bmi struct {
		Size                   uint32
		Width, Height          int32
		Planes, Bits           uint16
		Compression, ImageSize uint32
		XPels, YPels           int32
		Used, Important        uint32
		Colors                 [4]byte
	}
	bmi.Size = 40
	bmi.Width = 48
	bmi.Height = -48
	bmi.Planes = 1
	bmi.Bits = 32
	var bits unsafe.Pointer
	bitmap, _, _ := iconGDI.NewProc("CreateDIBSection").Call(dc, uintptr(unsafe.Pointer(&bmi)), 0, uintptr(unsafe.Pointer(&bits)), 0, 0)
	if bitmap != 0 {
		defer iconGDI.NewProc("DeleteObject").Call(bitmap)
	}
	if bitmap == 0 || bits == nil {
		return nil
	}
	old, _, _ := iconGDI.NewProc("SelectObject").Call(dc, bitmap)
	if old == 0 || old == ^uintptr(0) {
		return nil
	}
	pixels := unsafe.Slice((*byte)(bits), 48*48*4)
	clear(pixels)
	ok, _, _ := iconUser.NewProc("DrawIconEx").Call(dc, 0, 0, h, 48, 48, 0, 0, 3)
	iconGDI.NewProc("SelectObject").Call(dc, old)
	if ok == 0 {
		return nil
	}
	return iconEncode(&image.NRGBA{Pix: iconBGRA(pixels), Stride: 48 * 4, Rect: image.Rect(0, 0, 48, 48)})
}
