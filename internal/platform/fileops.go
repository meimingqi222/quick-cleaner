package platform

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
)

// ErrTrashUnsupported means no safe trash operation is available. Callers must
// report this error, never substitute permanent deletion.
var ErrTrashUnsupported = errors.New("safe trash operation unavailable")

// CurrentUID is the real user id whose per-volume trash directory is authoritative.
func CurrentUID() string         { return currentUID() }
func TrashDirectories() []string { return trashDirectories() }

func trashPath(path string) (string, error) {
	if path == "" || strings.ContainsRune(path, 0) {
		return "", errors.New("invalid trash path")
	}
	p, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	if _, err = os.Lstat(p); err != nil {
		return "", err
	}
	return p, nil
}

func logicalFileSize(info os.FileInfo) uint64 {
	if info == nil || info.Size() < 0 {
		return 0
	}
	return uint64(info.Size())
}
