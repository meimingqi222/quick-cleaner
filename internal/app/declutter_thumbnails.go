package app

import (
	"bytes"
	"context"
	"image"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

const (
	declutterPreviewBytes       = 8_000_000
	declutterPreviewPixels      = 16_000_000
	declutterPreviewTotalPixels = 32_000_000
	declutterPreviewTotalBytes  = 64_000_000
	declutterPreviewAttempts    = 128
)

// Preview work is sequential and bounded independently of the deletion snapshot.
// Originals remain the authority; unsupported/oversized previews retain fallback
// cards. Cancellation stops reads and publication, not an in-flight bounded decode.
func loadDeclutterThumbnails(ctx context.Context, groups []core.PhotoGroup) map[string]*ui.Bitmap {
	out := map[string]*ui.Bitmap{}
	seen := map[string]bool{}
	attempts, pixels, encoded := 0, 0, 0
	for _, group := range groups {
		for _, photo := range group.Photos {
			if ctx.Err() != nil || attempts >= declutterPreviewAttempts || encoded >= declutterPreviewTotalBytes {
				return out
			}
			if seen[photo.Path] || photo.Size > declutterPreviewBytes {
				continue
			}
			switch strings.ToLower(filepath.Ext(photo.Path)) {
			case ".jpg", ".jpeg", ".png", ".webp", ".bmp", ".gif":
			default:
				continue
			}
			seen[photo.Path] = true
			attempts++
			data := readDeclutterPreview(ctx, photo.Path, min(declutterPreviewBytes, declutterPreviewTotalBytes-encoded))
			encoded += len(data)
			config, _, err := image.DecodeConfig(bytes.NewReader(data))
			if err != nil || config.Width <= 0 || config.Height <= 0 || config.Width > declutterPreviewPixels/config.Height {
				continue
			}
			n := config.Width * config.Height
			if n > declutterPreviewTotalPixels-pixels || ctx.Err() != nil {
				continue
			}
			// Charge attempted decodes as well, so malformed inputs cannot repeatedly
			// allocate the entire aggregate budget.
			pixels += n
			bitmap, err := ui.DecodeBitmap(data)
			if err == nil && ctx.Err() == nil {
				out[photo.Path] = bitmap
			}
		}
	}
	return out
}

func readDeclutterPreview(ctx context.Context, path string, limit int) []byte {
	if ctx.Err() != nil {
		return nil
	}
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Size() > int64(limit) {
		return nil
	}
	f, err := os.Open(path)
	if err != nil {
		return nil
	}
	defer f.Close()
	opened, err := f.Stat()
	if err != nil || !opened.Mode().IsRegular() || !os.SameFile(info, opened) || opened.Size() > int64(limit) {
		return nil
	}
	data, err := io.ReadAll(io.LimitReader(declutterPreviewReader{ctx, f}, int64(limit)+1))
	if err != nil || len(data) > limit || ctx.Err() != nil {
		return nil
	}
	return data
}

type declutterPreviewReader struct {
	ctx context.Context
	r   io.Reader
}

func (r declutterPreviewReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.r.Read(p)
}
