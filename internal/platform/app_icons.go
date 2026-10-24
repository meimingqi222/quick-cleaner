package platform

import (
	"bytes"
	"encoding/binary"
	"image"
	"image/color"
	_ "image/gif"
	_ "image/jpeg"
	"image/png"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"unicode/utf16"
	"unicode/utf8"

	_ "golang.org/x/image/bmp"
	"golang.org/x/image/draw"
	_ "golang.org/x/image/webp"
)

func iconRegular(path string) bool { s, e := os.Stat(path); return e == nil && s.Mode().IsRegular() }

func iconASCIILower(s string) string {
	b := []byte(s)
	for i, c := range b {
		if c >= 'A' && c <= 'Z' {
			b[i] += 'a' - 'A'
		}
	}
	return string(b)
}

func iconEncode(img image.Image) []byte {
	var out bytes.Buffer
	if png.Encode(&out, img) != nil {
		return nil
	}
	return out.Bytes()
}

func iconResize(data []byte, size int, preserve bool) []byte {
	img, _, err := image.Decode(bytes.NewReader(data))
	if err != nil {
		if preserve {
			return data
		}
		return nil
	}
	b := img.Bounds()
	src := image.NewNRGBA(image.Rect(0, 0, b.Dx(), b.Dy()))
	for y := 0; y < b.Dy(); y++ {
		for x := 0; x < b.Dx(); x++ {
			src.SetNRGBA(x, y, color.NRGBAModel.Convert(img.At(b.Min.X+x, b.Min.Y+y)).(color.NRGBA))
		}
	}
	if b.Dx() > 256 || b.Dy() > 256 {
		dst := image.NewNRGBA(image.Rect(0, 0, size, size))
		// Rust resizes unassociated RGBA channels, including hidden RGB at
		// alpha zero. Scaling NRGBA directly would premultiply those away.
		plane := image.NewGray(src.Bounds())
		scaled := image.NewGray(dst.Bounds())
		for channel := 0; channel < 4; channel++ {
			for i := range plane.Pix {
				plane.Pix[i] = src.Pix[i*4+channel]
			}
			draw.BiLinear.Scale(scaled, scaled.Bounds(), plane, plane.Bounds(), draw.Src, nil)
			for i, v := range scaled.Pix {
				dst.Pix[i*4+channel] = v
			}
		}
		return iconEncode(dst)
	}
	if preserve {
		return data
	}
	return iconEncode(src)
}

func iconMacDownscale(data []byte) []byte {
	if len(data) >= 24 && bytes.HasPrefix(data, []byte("\x89PNG")) && string(data[12:16]) == "IHDR" && (binary.BigEndian.Uint32(data[16:20]) > 256 || binary.BigEndian.Uint32(data[20:24]) > 256) {
		return iconResize(data, 64, true)
	}
	return data
}

func iconICNS(data []byte) []byte {
	if len(data) < 8 || string(data[:4]) != "icns" {
		return nil
	}
	end := min(uint64(binary.BigEndian.Uint32(data[4:8])), uint64(len(data)))
	var best []byte
	score := 6
	for off := uint64(8); off+8 <= end; {
		n := uint64(binary.BigEndian.Uint32(data[off+4 : off+8]))
		if n < 8 || n > end-off {
			break
		}
		typ := string(data[off : off+4])
		payload := data[off+8 : off+n]
		off += n
		if !bytes.HasPrefix(payload, []byte("\x89PNG")) {
			continue
		}
		s := 6
		switch typ {
		case "ic12", "icp6":
			s = 0
		case "ic07":
			s = 1
		case "ic13", "ic08":
			s = 2
		case "ic11", "icp5":
			s = 3
		case "ic14", "ic09":
			s = 4
		case "ic10":
			s = 5
		}
		if s < score {
			score = s
			best = payload
			if s == 0 {
				break
			}
		}
	}
	return best
}

func iconPlist(data []byte, key string) string {
	if !bytes.HasPrefix(data, []byte("bplist")) {
		if !utf8.Valid(data) {
			return ""
		}
		_, rest, ok := strings.Cut(string(data), "<key>"+key+"</key>")
		if !ok {
			return ""
		}
		rest = strings.TrimSpace(rest)
		rest, ok = strings.CutPrefix(rest, "<string>")
		if !ok {
			return ""
		}
		value, _, _ := strings.Cut(rest, "</string>")
		return strings.TrimSpace(value)
	}
	if len(data) < 40 {
		return ""
	}
	t := data[len(data)-32:]
	osz, rsz := uint64(t[6]), uint64(t[7])
	if osz < 1 || osz > 8 || rsz < 1 || rsz > 8 {
		return ""
	}
	read := func(b []byte, off, n uint64) (uint64, bool) {
		if n < 1 || n > 8 || off > uint64(len(b)) || n > uint64(len(b))-off {
			return 0, false
		}
		var v uint64
		for _, x := range b[off : off+n] {
			v = v<<8 | uint64(x)
		}
		return v, true
	}
	count := binary.BigEndian.Uint64(t[8:16])
	top := binary.BigEndian.Uint64(t[16:24])
	table := binary.BigEndian.Uint64(t[24:32])
	if table > uint64(len(data)-32) || count > (uint64(len(data)-32)-table)/osz {
		return ""
	}
	obj := func(i uint64) (uint64, bool) {
		if i >= count || table > uint64(len(data)) || i > (uint64(len(data))-table)/osz {
			return 0, false
		}
		o, ok := read(data, table+i*osz, osz)
		return o, ok && o < uint64(len(data))
	}
	sized := func(off uint64) (uint64, uint64, bool) {
		n := uint64(data[off] & 15)
		if n < 15 {
			return n, off + 1, true
		}
		if off+2 > uint64(len(data)) || data[off+1]>>4 != 1 || data[off+1]&15 > 3 {
			return 0, 0, false
		}
		width := uint64(1) << uint(data[off+1]&15)
		n, ok := read(data, off+2, width)
		return n, off + 2 + width, ok
	}
	str := func(i uint64) string {
		off, ok := obj(i)
		if !ok {
			return ""
		}
		kind := data[off] >> 4
		n, p, ok := sized(off)
		if !ok || p > uint64(len(data)) {
			return ""
		}
		width := uint64(1)
		if kind == 6 {
			width = 2
		} else if kind != 5 {
			return ""
		}
		if n > (uint64(len(data))-p)/width {
			return ""
		}
		b := data[p : p+n*width]
		if kind == 5 {
			if !utf8.Valid(b) {
				return ""
			}
			return string(b)
		}
		u := make([]uint16, n)
		for j := range u {
			u[j] = binary.BigEndian.Uint16(b[j*2:])
		}
		for j := 0; j < len(u); j++ {
			if u[j] >= 0xd800 && u[j] <= 0xdbff {
				if j+1 >= len(u) || u[j+1] < 0xdc00 || u[j+1] > 0xdfff {
					return ""
				}
				j++
			} else if u[j] >= 0xdc00 && u[j] <= 0xdfff {
				return ""
			}
		}
		return string(utf16.Decode(u))
	}
	off, ok := obj(top)
	if !ok || data[off]>>4 != 13 {
		return ""
	}
	n, p, ok := sized(off)
	if !ok || p > uint64(len(data)) || n > (uint64(len(data))-p)/rsz/2 {
		return ""
	}
	for i := uint64(0); i < n; i++ {
		k, _ := read(data, p+i*rsz, rsz)
		if str(k) == key {
			v, _ := read(data, p+(n+i)*rsz, rsz)
			return str(v)
		}
	}
	return ""
}

func iconMacBundle(app string) []byte {
	r := filepath.Join(app, "Contents", "Resources")
	var selected string
	data, _ := os.ReadFile(filepath.Join(app, "Contents", "Info.plist"))
	name := strings.TrimSpace(iconPlist(data, "CFBundleIconFile"))
	if name != "" {
		for _, suffix := range []string{"", ".icns", ".png"} {
			if suffix != "" && strings.HasSuffix(name, suffix) {
				continue
			}
			p := filepath.Join(r, name+suffix)
			if filepath.IsAbs(name) {
				p = name + suffix
			}
			if iconRegular(p) {
				selected = p
				break
			}
		}
	}
	if selected == "" {
		stem := strings.TrimSuffix(filepath.Base(app), filepath.Ext(app))
		for _, name := range []string{"AppIcon.icns", "icon.icns", "app.icns", "Icon.icns", stem + ".icns"} {
			p := filepath.Join(r, name)
			if iconRegular(p) {
				selected = p
				break
			}
		}
	}
	if selected == "" {
		entries, _ := os.ReadDir(r)
		count := 0
		for _, e := range entries {
			if iconASCIILower(filepath.Ext(e.Name())) != ".icns" {
				continue
			}
			lower := iconASCIILower(strings.TrimSuffix(e.Name(), filepath.Ext(e.Name())))
			skip := false
			for _, s := range []string{"document", "badge", "alias", "folder"} {
				skip = skip || strings.Contains(lower, s)
			}
			if !skip {
				count++
				selected = filepath.Join(r, e.Name())
			}
		}
		if count != 1 {
			selected = ""
		}
	}
	if selected != "" {
		if b, e := os.ReadFile(selected); e == nil {
			if iconASCIILower(filepath.Ext(selected)) == ".png" {
				return iconMacDownscale(b)
			}
			if png := iconICNS(b); png != nil {
				return iconMacDownscale(png)
			}
		}
	}
	for _, name := range []string{"assets/icons/64x64.png", "assets/icons/128x128.png", "assets/icon.png", "icon.png", "AppIcon.png"} {
		if b, e := os.ReadFile(filepath.Join(r, name)); e == nil && bytes.HasPrefix(b, []byte("\x89PNG")) {
			return iconMacDownscale(b)
		}
	}
	return nil
}

// DisplayIcon resource indices select no alternate icon in the Rust source;
// normalize the registration hint, then extract the module's default icon.
func iconDisplayPath(path string) string {
	path = strings.Trim(strings.TrimSpace(path), "\"")
	if i := strings.LastIndexByte(path, ','); i >= 0 {
		suffix := strings.TrimSpace(path[i+1:])
		numeric := suffix != ""
		for _, c := range suffix {
			numeric = numeric && (c >= '0' && c <= '9' || c == '-')
		}
		if numeric {
			path = strings.TrimSpace(path[:i])
		}
	}
	return strings.Trim(path, "\"")
}

func iconWindowsCandidates(dir string) []string {
	stem := filepath.Base(dir)
	var out []string
	for _, name := range []string{stem + ".exe", stem + ".ico", filepath.Join("icons", stem+".ico"), "icon.ico", filepath.Join("bin", stem+".exe"), filepath.Join("application", stem+".exe"), filepath.Join("app", stem+".exe"), filepath.Join("program", stem+".exe")} {
		p := filepath.Join(dir, name)
		if iconRegular(p) {
			out = append(out, p)
		}
	}
	entries, _ := os.ReadDir(dir)
	var others []string
	for _, e := range entries {
		lower := iconASCIILower(e.Name())
		if !strings.HasSuffix(lower, ".exe") {
			continue
		}
		skip := false
		for _, s := range []string{"unins", "uninstall", "setup", "update", "crashpad", "crash_handler", "helper", "installer", "maintenance", "elevate"} {
			skip = skip || strings.Contains(lower, s)
		}
		p := filepath.Join(dir, e.Name())
		for _, q := range out {
			skip = skip || iconASCIILower(p) == iconASCIILower(q)
		}
		if !skip && iconRegular(p) {
			others = append(others, p)
		}
	}
	key := func(p string) string { return iconASCIILower(strings.TrimSuffix(filepath.Base(p), filepath.Ext(p))) }
	sort.SliceStable(others, func(i, j int) bool {
		a, b := key(others[i]), key(others[j])
		am, bm := strings.Contains(a, iconASCIILower(stem)), strings.Contains(b, iconASCIILower(stem))
		if am != bm {
			return am
		}
		return a < b
	})
	return append(out, others...)
}

func iconWindowsImage(path string) []byte {
	switch iconASCIILower(filepath.Ext(path)) {
	case ".png", ".jpg", ".jpeg", ".bmp", ".gif", ".webp":
		b, e := os.ReadFile(path)
		if e == nil {
			return iconResize(b, 48, false)
		}
	}
	return nil
}

func iconBGRA(data []byte) []byte {
	rgba := append([]byte(nil), data...)
	allZero := true
	for i := 0; i+3 < len(rgba); i += 4 {
		rgba[i], rgba[i+2] = rgba[i+2], rgba[i]
		allZero = allZero && rgba[i+3] == 0
	}
	if allZero {
		for i := 3; i < len(rgba); i += 4 {
			rgba[i] = 255
		}
	}
	return rgba
}
