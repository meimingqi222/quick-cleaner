// Command winres writes the Windows resources of quick-cleaner.exe as a .syso
// the Go linker links into the main package: the application icon (group 1,
// which the MyGo window class loads), version information and the manifest.
// The values match the Rust build.rs winres configuration, so Explorer, the
// taskbar and UAC see the same program.
//
// The COFF resource layout follows MyGo's cmd/mygo/winres.go (MIT License,
// Copyright (c) 2026 MyGo contributors). MyGo's own manifest is not used: it
// omits Rust's asInvoker and longPathAware declarations.
//
//	go run ./tools/winres                 # write cmd/quick-cleaner/rsrc_windows_amd64.syso
//	go run ./tools/winres -check          # fail if the committed file is stale
package main

import (
	"bytes"
	"encoding/binary"
	"errors"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strconv"
	"strings"
	"unicode/utf16"
)

const (
	rtIcon      = 3
	rtGroupIcon = 14
	rtVersion   = 16
	rtManifest  = 24
	// LANG_NEUTRAL/SUBLANG_NEUTRAL, as winres writes for the Rust build.
	langNeutral = 0
)

// manifest is build.rs's manifest, unchanged.
const manifest = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
    <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
        <security>
            <requestedPrivileges>
                <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
            </requestedPrivileges>
        </security>
    </trustInfo>
    <application xmlns="urn:schemas-microsoft-com:asm.v3">
        <windowsSettings>
            <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
            <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
            <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
        </windowsSettings>
    </application>
    <dependency>
        <dependentAssembly>
            <assemblyIdentity
                type="win32"
                name="Microsoft.Windows.Common-Controls"
                version="6.0.0.0"
                processorArchitecture="*"
                publicKeyToken="6595b64144ccf1df"
                language="*"
            />
        </dependentAssembly>
    </dependency>
</assembly>
`

func main() {
	root := flag.String("root", ".", "repository root")
	arch := flag.String("arch", "amd64", "GOARCH of the executable")
	check := flag.Bool("check", false, "verify the committed .syso instead of writing it")
	flag.Parse()
	if err := run(*root, *arch, *check); err != nil {
		fmt.Fprintln(os.Stderr, "winres:", err)
		os.Exit(1)
	}
}

func run(root, arch string, check bool) error {
	version, err := appVersion(filepath.Join(root, "internal", "app", "root.go"))
	if err != nil {
		return err
	}
	icon, err := os.ReadFile(filepath.Join(root, "assets", "icon.ico"))
	if err != nil {
		return err
	}
	syso, err := build(icon, version, arch)
	if err != nil {
		return err
	}
	out := filepath.Join(root, "cmd", "quick-cleaner", "rsrc_windows_"+arch+".syso")
	if check {
		old, err := os.ReadFile(out)
		if err != nil {
			return err
		}
		if !bytes.Equal(old, syso) {
			return fmt.Errorf("%s is stale for version %s; run go run ./tools/winres", out, version)
		}
		return nil
	}
	return os.WriteFile(out, syso, 0o644)
}

var versionConst = regexp.MustCompile(`(?m)^const Version = "([0-9]+(?:\.[0-9]+){0,3})"\r?$`)

// appVersion reads the single source of the product version.
func appVersion(path string) (string, error) {
	src, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	m := versionConst.FindSubmatch(src)
	if m == nil {
		return "", fmt.Errorf("no numeric const Version in %s", path)
	}
	return string(m[1]), nil
}

func build(icon []byte, version, arch string) ([]byte, error) {
	images, group, err := iconResources(icon)
	if err != nil {
		return nil, fmt.Errorf("assets/icon.ico: %w", err)
	}
	res := map[uint16]map[uint16][]byte{
		rtIcon:      images,
		rtGroupIcon: {1: group},
		rtVersion:   {1: versionInfo(version)},
		rtManifest:  {1: []byte(manifest)},
	}
	return coff(res, arch)
}

// iconResources splits an .ico file into RT_ICON images (ids 1..n) and the
// RT_GROUP_ICON directory that names them. The group differs from the file
// header only in replacing each image's file offset with its resource id.
func iconResources(ico []byte) (map[uint16][]byte, []byte, error) {
	le := binary.LittleEndian
	if len(ico) < 6 || le.Uint16(ico[0:]) != 0 || le.Uint16(ico[2:]) != 1 {
		return nil, nil, errors.New("not an icon file")
	}
	n := int(le.Uint16(ico[4:]))
	if n == 0 || len(ico) < 6+16*n {
		return nil, nil, errors.New("truncated icon directory")
	}
	images := map[uint16][]byte{}
	group := make([]byte, 6, 6+14*n)
	copy(group, ico[:6])
	for i := range n {
		e := ico[6+16*i : 6+16*(i+1)]
		size, off := le.Uint32(e[8:]), le.Uint32(e[12:])
		if uint64(off)+uint64(size) > uint64(len(ico)) {
			return nil, nil, fmt.Errorf("image %d outside the file", i)
		}
		id := uint16(i + 1)
		images[id] = ico[off : off+size]
		group = append(group, e[:12]...)
		group = le.AppendUint16(group, id)
	}
	return images, group, nil
}

// fileVersion turns "0.0.16" into the four VS_FIXEDFILEINFO numbers.
func fileVersion(v string) [4]uint16 {
	var out [4]uint16
	for i, part := range strings.SplitN(v, ".", 4) {
		n, _ := strconv.ParseUint(part, 10, 16)
		out[i] = uint16(n)
	}
	return out
}

// versionInfo holds build.rs's strings plus the version, in one
// language-neutral Unicode table as winres writes it.
func versionInfo(version string) []byte {
	v := fileVersion(version)
	le := binary.LittleEndian
	fixed := make([]byte, 52)
	le.PutUint32(fixed[0:], 0xFEEF04BD)
	le.PutUint32(fixed[4:], 0x00010000)
	le.PutUint32(fixed[8:], uint32(v[0])<<16|uint32(v[1]))
	le.PutUint32(fixed[12:], uint32(v[2])<<16|uint32(v[3]))
	copy(fixed[16:24], fixed[8:16])
	le.PutUint32(fixed[24:], 0x3F)
	le.PutUint32(fixed[32:], 0x00040004) // VOS_NT_WINDOWS32
	le.PutUint32(fixed[36:], 1)          // VFT_APP
	var table []verNode
	for _, s := range [][2]string{
		{"FileDescription", "QuickCleaner 极速磁盘分析与清理工具"},
		{"FileVersion", version},
		{"InternalName", "quick-cleaner"},
		{"LegalCopyright", "Copyright © 2026 QuickCleaner"},
		{"OriginalFilename", "quick-cleaner.exe"},
		{"ProductName", "QuickCleaner"},
		{"ProductVersion", version},
	} {
		text := utf16.Encode([]rune(s[1] + "\x00"))
		value := make([]byte, 2*len(text))
		for i, u := range text {
			le.PutUint16(value[2*i:], u)
		}
		table = append(table, verNode{key: s[0], text: true, value: value, valueLen: len(text)})
	}
	return verNode{key: "VS_VERSION_INFO", value: fixed, valueLen: len(fixed), children: []verNode{
		{key: "StringFileInfo", text: true, children: []verNode{{key: "000004b0", text: true, children: table}}},
		{key: "VarFileInfo", text: true, children: []verNode{{key: "Translation", value: []byte{0, 0, 0xB0, 0x04}, valueLen: 4}}},
	}}.encode()
}

type verNode struct {
	key      string
	text     bool
	value    []byte
	valueLen int // bytes for binary values, characters for text
	children []verNode
}

func (n verNode) encode() []byte {
	b := make([]byte, 6)
	for _, u := range utf16.Encode([]rune(n.key + "\x00")) {
		b = binary.LittleEndian.AppendUint16(b, u)
	}
	pad := func() {
		for len(b)%4 != 0 {
			b = append(b, 0)
		}
	}
	pad()
	b = append(b, n.value...)
	for _, child := range n.children {
		pad()
		b = append(b, child.encode()...)
	}
	binary.LittleEndian.PutUint16(b[0:], uint16(len(b)))
	binary.LittleEndian.PutUint16(b[2:], uint16(n.valueLen))
	if n.text {
		binary.LittleEndian.PutUint16(b[4:], 1)
	}
	return b
}

// coff lays out a .rsrc section: type, id and language directories, data
// entries whose RVAs the linker fills from relocations, then the data.
func coff(res map[uint16]map[uint16][]byte, goarch string) ([]byte, error) {
	arch, ok := map[string]struct{ machine, relocation uint16 }{
		"amd64": {0x8664, 0x0003}, // IMAGE_REL_AMD64_ADDR32NB
		"arm64": {0xAA64, 0x0002}, // IMAGE_REL_ARM64_ADDR32NB
		"386":   {0x014C, 0x0007}, // IMAGE_REL_I386_DIR32NB
	}[goarch]
	if !ok {
		return nil, fmt.Errorf("no Windows resources for %s", goarch)
	}
	sorted := func(m map[uint16][]byte) []uint16 {
		keys := make([]uint16, 0, len(m))
		for k := range m {
			keys = append(keys, k)
		}
		slices.Sort(keys)
		return keys
	}
	types := make([]uint16, 0, len(res))
	for t := range res {
		types = append(types, t)
	}
	slices.Sort(types)
	dirSize := func(n int) int { return 16 + 8*n }
	size := dirSize(len(types))
	count := 0
	for _, t := range types {
		size += dirSize(len(res[t])) + len(res[t])*dirSize(1)
		count += len(res[t])
	}
	entriesAt := size
	dataAt := entriesAt + 16*count
	raw := make([]byte, dataAt)
	le := binary.LittleEndian
	dir := func(at, n int) { le.PutUint16(raw[at+14:], uint16(n)) }
	var relocs []uint32
	var data bytes.Buffer
	dir(0, len(types))
	next := dirSize(len(types))
	entry := entriesAt
	for i, t := range types {
		le.PutUint32(raw[16+8*i:], uint32(t))
		le.PutUint32(raw[16+8*i+4:], uint32(next)|0x80000000)
		ids := sorted(res[t])
		idDir := next
		dir(idDir, len(ids))
		next += dirSize(len(ids))
		for j, id := range ids {
			le.PutUint32(raw[idDir+16+8*j:], uint32(id))
			le.PutUint32(raw[idDir+16+8*j+4:], uint32(next)|0x80000000)
			dir(next, 1)
			le.PutUint32(raw[next+16:], langNeutral)
			le.PutUint32(raw[next+20:], uint32(entry))
			next += dirSize(1)
			for data.Len()%8 != 0 {
				data.WriteByte(0)
			}
			le.PutUint32(raw[entry:], uint32(dataAt+data.Len()))
			le.PutUint32(raw[entry+4:], uint32(len(res[t][id])))
			relocs = append(relocs, uint32(entry))
			entry += 16
			data.Write(res[t][id])
		}
	}
	raw = append(raw, data.Bytes()...)
	for len(raw)%4 != 0 {
		raw = append(raw, 0)
	}
	var out bytes.Buffer
	const headerSize, sectionHeaderSize = 20, 40
	rawAt := headerSize + sectionHeaderSize
	relocAt := rawAt + len(raw)
	symbolsAt := relocAt + 10*len(relocs)
	_ = binary.Write(&out, le, struct {
		Machine, Sections                   uint16
		Time, Symbols, SymbolCount          uint32
		OptionalHeaderSize, Characteristics uint16
	}{arch.machine, 1, 0, uint32(symbolsAt), 1, 0, 0})
	_ = binary.Write(&out, le, struct {
		Name                                        [8]byte
		VirtualSize, VirtualAddress, RawSize, RawAt uint32
		RelocAt, LineNumbersAt                      uint32
		Relocs, LineNumbers                         uint16
		Characteristics                             uint32
	}{[8]byte{'.', 'r', 's', 'r', 'c'}, 0, 0, uint32(len(raw)), uint32(rawAt), uint32(relocAt), 0, uint16(len(relocs)), 0, 0x40000040})
	out.Write(raw)
	for _, r := range relocs {
		_ = binary.Write(&out, le, r)
		_ = binary.Write(&out, le, uint32(0))
		_ = binary.Write(&out, le, arch.relocation)
	}
	out.Write([]byte{'.', 'r', 's', 'r', 'c', 0, 0, 0})
	_ = binary.Write(&out, le, uint32(0))
	_ = binary.Write(&out, le, uint16(1))
	_ = binary.Write(&out, le, uint16(0))
	out.Write([]byte{3, 0})
	_ = binary.Write(&out, le, uint32(4))
	return out.Bytes(), nil
}
