package ui

import (
	"strings"

	"github.com/egoist/mygo/ui"
)

// Port the original box geometry, including asymmetric corner radii.
func icon(c *ui.Context, view View, fg ui.Color, size float32) {
	switch view {
	case Overview:
		half := (size - 3) / 2
		ui.Column(c).Size(size, size).Shrink(0).Gap(0).Justify(ui.SpaceBetween).Children(func() {
			for row := 0; row < 2; row++ {
				ui.Row(c).FillWidth().Gap(0).Justify(ui.SpaceBetween).Children(func() {
					for col := 0; col < 2; col++ {
						color := fg
						if row != col {
							color = fg.Alpha(.6)
						}
						ui.Box(c).Size(half, half).Radius(2).Background(color)
					}
				})
			}
		})
	case Status:
		w := (size - 4) / 3
		ui.Row(c).Size(size, size).Shrink(0).Gap(2).AlignItems(ui.End).Justify(ui.Center).Children(func() {
			for _, h := range []float32{size * .45, size, size * .7} {
				ui.Box(c).Size(w, h).Radius(w, w, 0, 0).Background(fg)
			}
		})
	case Junk:
		ui.Column(c).Size(size, size).Shrink(0).Center().Gap(1.5).Children(func() {
			ui.Box(c).Size(size*.9, 2.5).Radius(999).Background(fg)
			ui.Row(c).Size(size*.75, size*.7).Radius(0, 0, 3, 3).Border(2, fg).Center().Gap(2).Children(func() {
				for i := 0; i < 2; i++ {
					ui.Box(c).Size(1.5, size*.7*.55).Radius(999).Background(fg)
				}
			})
		})
	case Apps:
		dot := (size - 4) / 3
		ui.Column(c).Size(size, size).Shrink(0).Gap(0).Justify(ui.SpaceBetween).Children(func() {
			for row := 0; row < 3; row++ {
				ui.Row(c).FillWidth().Justify(ui.SpaceBetween).Gap(0).Children(func() {
					for col := 0; col < 3; col++ {
						ui.Box(c).Size(dot, dot).Radius(1.5).Background(fg)
					}
				})
			}
		})
	case Disk:
		ui.Row(c).Size(size, size*.75).Shrink(0).Radius(6).Border(2, fg).Padding(2).Justify(ui.SpaceBetween).Children(func() {
			ui.Box(c).Size(size*.35, size*.35).Radius(999).Border(2, fg)
			ui.Box(c).Size(4, 4).Radius(999).Background(fg)
		})
	case Declutter:
		ui.Box(c).Size(size, size).Shrink(0).Center().Children(func() {
			ui.Column(c).Size(size*.75, size*.75).Radius(6).Border(2, fg).Padding(2).Gap(0).Justify(ui.SpaceBetween).Children(func() {
				ui.Box(c).FillWidth().Height(2).Radius(999).Background(fg)
				ui.Box(c).Size(size*.35, 2).Radius(999).Background(fg)
			})
			ui.Box(c).Absolute().Right(0).Top(0).Size(size*.35, size*.35).Radius(999).Background(primaryBright)
		})
	case Search:
		ui.Box(c).Size(size, size).Shrink(0).Children(func() {
			ui.Box(c).Size(size*.65, size*.65).Radius(999).Border(2, fg)
			ui.Box(c).Absolute().Right(1).Bottom(1).Size(size*.45, 2).Radius(999).Background(fg)
		})
	}
}

func shield(c *ui.Context, fg ui.Color) {
	ui.Box(c).Size(12, 12).Shrink(0).Border(2, fg).Radius(6, 6, 12, 12).Center().Children(func() {
		ui.Box(c).Size(4.2, 4.2).Radius(2).Background(fg)
	})
}

type FileVisualKind uint8

const (
	FileDirectory FileVisualKind = iota
	FileExecutable
	FileDatabase
	FileCode
	FileDocument
	FileArchive
	FileMedia
	FileGeneric
)

// FileKind retains the source's final-segment classification, even without a dot.
func FileKind(name string, isDir bool) FileVisualKind {
	if isDir {
		return FileDirectory
	}
	ext := strings.Map(func(r rune) rune {
		if r >= 'A' && r <= 'Z' {
			return r + ('a' - 'A')
		}
		return r
	}, name[strings.LastIndexByte(name, '.')+1:])
	switch ext {
	case "exe", "msi", "bat", "cmd", "ps1", "vbs", "com", "dll", "sys":
		return FileExecutable
	case "db", "sqlite", "sqlite3", "dat", "bin", "sql", "mdb", "accdb", "db3", "rdb":
		return FileDatabase
	case "rs", "go", "py", "js", "ts", "jsx", "tsx", "c", "cpp", "h", "hpp", "java", "kt", "swift", "rb", "php", "html", "css", "scss", "json", "yaml", "yml", "toml", "xml", "md", "sh", "lua", "vue":
		return FileCode
	case "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "log", "rtf", "csv", "ini", "cfg", "conf":
		return FileDocument
	case "zip", "7z", "rar", "tar", "gz", "bz2", "xz", "iso", "cab", "tgz":
		return FileArchive
	case "png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "bmp", "mp4", "mkv", "avi", "mov", "mp3", "wav", "flac", "aac", "webm":
		return FileMedia
	default:
		return FileGeneric
	}
}

func (kind FileVisualKind) Render(c *ui.Context, size float32) {
	fg := ui.Hex([]string{"#D97706", "#7C3AED", "#0891B2", "#059669", "#2563EB", "#C2410C", "#DB2777", "#64748B"}[kind])
	ui.Box(c).Size(size, size).Shrink(0).Center().Children(func() {
		switch kind {
		case FileDirectory:
			ui.Column(c).Width(size * .88).Gap(0).Children(func() {
				ui.Box(c).Size(size*.88*.44, 2.5).Radius(2, 2, 0, 0).Background(fg)
				ui.Box(c).Size(size*.88, size*.65).Radius(0, 3, 3, 3).Border(1, fg)
			})
		case FileExecutable:
			ui.Box(c).Size(size*.78, size*.78).Radius(4).Border(1, fg).Background(fg.Alpha(.2)).Center().Children(func() {
				ui.Box(c).Size(size*.78*.45, size*.78*.45).Radius(1).Background(fg)
			})
		case FileDatabase:
			ui.Column(c).Size(size*.75, size*.85).Gap(0).Justify(ui.SpaceBetween).Children(func() {
				for i := 0; i < 3; i++ {
					e := ui.Box(c).Size(size*.75, size*.85*.28).Border(1, fg).Background(fg.Alpha(.2))
					if i == 0 {
						e.Radius(999)
					} else {
						e.Radius(0, 0, 999, 999).BorderWidth(0, 1, 1, 1)
					}
				}
			})
		case FileArchive:
			ui.Column(c).Size(size*.7, size*.85).Radius(4).Border(2, fg).Padding(2, 0).Gap(1.5).AlignItems(ui.Center).Children(func() {
				for i := 0; i < 3; i++ {
					ui.Box(c).Size(3, 1.5).Background(fg)
				}
				ui.Box(c).Size(6, 4).Radius(1).Background(fg)
			})
		case FileMedia:
			ui.Column(c).Size(size*.85, size*.7).Radius(4).Border(2, fg).Padding(2).Gap(0).Justify(ui.SpaceBetween).Children(func() {
				ui.Box(c).Size(3.5, 3.5).Radius(999).Background(fg)
				ui.Row(c).FillWidth().Height(size * .7 * .4).AlignItems(ui.End).Gap(2).Children(func() {
					ui.Box(c).Size(size*.85*.35, size*.7*.35).Radius(2, 2, 0, 0).Background(fg.Alpha(.8))
					ui.Box(c).Size(size*.85*.45, size*.7*.25).Radius(2, 2, 0, 0).Background(fg)
				})
			})
		default:
			width := size * .68
			if kind == FileCode {
				width = size * .72
			}
			ui.Column(c).Size(width, size*.85).Radius(2).Border(1, fg).Background(fg.Alpha(.15)).Padding(0, 2).Gap(1.5).Center().Children(func() {
				if kind == FileCode {
					ui.Text(c, "<>").FontSize(12).Bold().TextColor(fg)
				} else if kind == FileDocument {
					for _, scale := range []float32{1, .75, .5} {
						ui.Box(c).Size((width-4)*scale, 1.2).Radius(999).Background(fg).AlignSelf(ui.Start)
					}
				}
			})
		}
	})
}
