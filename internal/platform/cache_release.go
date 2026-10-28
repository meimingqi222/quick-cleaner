package platform

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/shirou/gopsutil/v4/process"
)

// Identity is a native Darwin uniqueid or an exact creation FILETIME, not
// gopsutil's rounded start time. Evidence identifies the occupied cache path.
type CacheOccupant struct {
	PID                        uint32
	Identity                   uint64
	Executable, Name, Evidence string
}

func cacheProcessSnapshot(pid uint32) (string, string, error) {
	if pid <= 1 || pid > 1<<31-1 {
		return "", "", errors.New("invalid-cache-owner")
	}
	p, err := process.NewProcess(int32(pid))
	if err != nil {
		return "", "", err
	}
	me, err := process.NewProcess(int32(os.Getpid()))
	if err != nil {
		return "", "", err
	}
	user, err := p.Username()
	if err != nil || user == "" {
		return "", "", errors.New("owner-user-unknown")
	}
	ownUser, err := me.Username()
	if err != nil || user != ownUser {
		return "", "", errors.New("system-or-other-user-process")
	}
	ancestor := p
	for depth := 0; ; depth++ {
		if depth >= 256 || ancestor.Pid == me.Pid {
			return "", "", errors.New("cleaner-process-tree-or-unknown-ancestry")
		}
		parent, err := ancestor.Ppid()
		if err != nil {
			return "", "", err
		}
		if parent <= 1 {
			break
		}
		ancestor, err = process.NewProcess(parent)
		if err != nil {
			return "", "", err
		}
	}
	exe, err := p.Exe()
	if err != nil || !filepath.IsAbs(exe) {
		return "", "", errors.New("owner-executable-unknown")
	}
	lower := strings.ToLower(strings.ReplaceAll(exe, `\`, "/"))
	if strings.HasPrefix(lower, "/system/") || strings.HasPrefix(lower, "/usr/libexec/") || strings.HasPrefix(lower, "/sbin/") || strings.HasPrefix(lower, "/usr/sbin/") || strings.Contains(lower, "/windows/") {
		return "", "", errors.New("system-process")
	}
	parent, err := p.Ppid()
	if err != nil {
		return "", "", err
	}
	if parent == 1 && !strings.Contains(lower, ".app/") {
		return "", "", errors.New("background-service")
	}
	name, err := p.Name()
	if err != nil {
		return "", "", err
	}
	return name, exe, nil
}

// Strict framing avoids interpreting a partial/truncated lsof response as vacancy.
func parseCacheLsof(data []byte, root string) (map[uint32]string, error) {
	owners := map[uint32]string{}
	if len(data) == 0 {
		return owners, nil
	}
	if len(data) > 16<<20 || !bytes.HasSuffix(data, []byte{0, '\n'}) || !utf8.Valid(data) {
		return nil, errors.New("cache-owner-probe-incomplete")
	}
	var pid uint32
	hadPath := false
	for _, field := range bytes.Split(data, []byte{0}) {
		field = bytes.TrimPrefix(field, []byte{'\n'})
		if len(field) == 0 {
			continue
		}
		switch field[0] {
		case 'p':
			if pid != 0 && !hadPath {
				return nil, errors.New("owner-path-missing")
			}
			n, err := strconv.ParseUint(string(field[1:]), 10, 32)
			if err != nil || n <= 1 {
				return nil, errors.New("invalid-owner-pid")
			}
			pid = uint32(n)
			hadPath = false
		case 'n':
			path := string(field[1:])
			if pid == 0 || !filepath.IsAbs(path) || filepath.Clean(path) != path {
				return nil, errors.New("invalid-owner-path")
			}
			hadPath = true
			rel, err := filepath.Rel(root, path)
			if err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
				if _, ok := owners[pid]; !ok {
					owners[pid] = path
				}
			}
		default:
			return nil, fmt.Errorf("unexpected-lsof-field: %q", field[0])
		}
	}
	if pid == 0 || !hadPath {
		return nil, errors.New("owner-path-missing")
	}
	return owners, nil
}
