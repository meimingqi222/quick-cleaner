package core

import (
	"context"
	"fmt"
	"path/filepath"
	"slices"
	"sort"
	"strings"
	"time"
)

type DownloadItem struct {
	Filename, Path        string
	Size, ModifiedAt      uint64
	DownloadedAtStr, Kind Text
	Selected              bool
}

type LargeFileItem struct {
	Filename, PathDisplay, Path string
	Size, LastAccessedSecs      uint64
	LastAccessedStr, Kind       Text
	IconType                    int
	Selected                    bool
}

func FormatAgeText(days uint64) Text {
	switch {
	case days == 0:
		return NewText("今天", "Today")
	case days == 1:
		return NewText("昨天", "Yesterday")
	case days < 30:
		return NewText(fmt.Sprintf("%d 天前", days), fmt.Sprintf("%d days ago", days))
	default:
		n, unit, zh := days/30, "month", "个月"
		if days >= 365 {
			n, unit, zh = days/365, "year", "年"
		}
		if n != 1 {
			unit += "s"
		}
		return NewText(fmt.Sprintf("%d %s前", n, zh), fmt.Sprintf("%d %s ago", n, unit))
	}
}

func formatTimestampDate(seconds uint64) string {
	// chrono's supported proleptic Gregorian range is smaller than Go's.
	signed := int64(seconds)
	if seconds == 0 || signed < -8334601228800 || signed > 8210266876799 {
		return "Unknown"
	}
	date := time.Unix(signed, 0).UTC()
	formatted := date.Format("2006-01-02")
	if date.Year() > 9999 {
		return "+" + formatted
	}
	return formatted
}

func declutterExtension(s *RuleSnapshot, policy, path string) bool {
	ext := strings.ToLower(strings.TrimPrefix(filepath.Ext(path), "."))
	return slices.Contains(s.List("engine", policy), ext)
}

// The caller supplies the trusted download folder, including redirection.
// Discovery is read-only; selected paths use the existing arbitrary/trash route.
func ScanDownloads(ctx context.Context, s *RuleSnapshot, dir string, query FileIndexQuery) []DownloadItem {
	if dir == "" {
		return nil
	}
	if query == nil {
		query = NewFSIndexEngine(nil)
	}
	files := query.QueryFiles(ctx, NewQueryFilter([]string{dir}).WithMaxDepth(int(s.Number("engine", "download_max_depth", 4))))
	now := uint64(max(0, time.Now().Unix()))
	var items []DownloadItem
	for _, file := range files {
		if ctx.Err() != nil {
			break
		}
		kind := NewText("其它文件", "Other")
		for _, row := range []struct{ policy, zh, en string }{{"download_installer", "安装包", "Installer"}, {"download_archive", "压缩包", "Archive"}, {"download_document", "文档", "Document"}, {"download_video", "视频", "Video"}, {"download_audio", "音频", "Audio"}, {"download_image", "图片", "Image"}} {
			if declutterExtension(s, row.policy, file.Path) {
				kind = NewText(row.zh, row.en)
				break
			}
		}
		age := NewText("刚刚", "Just now")
		if file.Mtime > 0 && now > file.Mtime {
			age = FormatAgeText((now - file.Mtime) / 86400)
		}
		items = append(items, DownloadItem{Filename: filepath.Base(file.Path), Path: file.Path, Size: file.Size, ModifiedAt: file.Mtime, DownloadedAtStr: age, Kind: kind})
	}
	sort.SliceStable(items, func(i, j int) bool { return items[i].ModifiedAt > items[j].ModifiedAt })
	return items
}

func ScanLargeOldFiles(ctx context.Context, s *RuleSnapshot, roots []string, minSize uint64, query FileIndexQuery) []LargeFileItem {
	if query == nil {
		query = NewFSIndexEngine(nil)
	}
	files := query.QueryLargeFiles(ctx, roots, minSize, int(s.Number("engine", "large_result_limit", 500)))
	now := uint64(max(0, time.Now().Unix()))
	var items []LargeFileItem
	for _, file := range files {
		if ctx.Err() != nil {
			break
		}
		kind, icon := NewText("其它文件", "Others"), 3
		for i, row := range []struct{ policy, zh, en string }{{"large_video", "视频影音", "Videos"}, {"large_archive", "归档文件", "Archives"}, {"large_document", "文本文档", "Documents"}} {
			if declutterExtension(s, row.policy, file.Path) {
				kind, icon = NewText(row.zh, row.en), i
				break
			}
		}
		days := saturatingSub(now, file.Mtime) / 86400
		// Preserve the reference's mtime-based "last accessed" presentation.
		items = append(items, LargeFileItem{Filename: filepath.Base(file.Path), PathDisplay: file.Path, Path: file.Path, Size: file.Size, LastAccessedSecs: file.Mtime, LastAccessedStr: FormatAgeText(days), Kind: kind, IconType: icon, Selected: days >= s.Number("engine", "large_selected_age_days", 90)})
	}
	sort.SliceStable(items, func(i, j int) bool { return items[i].Size > items[j].Size })
	return items
}
