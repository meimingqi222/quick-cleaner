package platform

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"testing"
)

// Rust spills raw scan entries once a scan passes 400,000 so the build can
// stream them into the v7 mapping instead of holding the raw array and the
// finished tree at the same time. The record layout is shared with that build:
// [parent u32][is_dir u8][mtime u32][size u64][name_len u16][name].
const macWalkSpillHeader = 19

// macWalkSpillThreshold matches Rust's 400,000. Tests lower it.
var macWalkSpillThreshold = 400_000

var (
	macWalkSpillSeq  atomic.Uint64
	macWalkSpillOnce sync.Once
)

type macWalkSpill struct {
	file       *os.File
	path       string
	n          uint64
	nameBytes  uint64
	failed     bool
	buf        []byte
	header     [macWalkSpillHeader]byte
	reclaimed  uint64
	reclaimLog string
}

func reclaimStaleMacWalkSpill() {
	if testing.Testing() {
		reclaimStaleMacWalkSpillNow()
		return
	}
	macWalkSpillOnce.Do(reclaimStaleMacWalkSpillNow)
}

func reclaimStaleMacWalkSpillNow() {
	dir, err := os.ReadDir(os.TempDir())
	if err != nil {
		return
	}
	var reclaimed, bytes int64
	for _, entry := range dir {
		pid, ok := parseMacWalkSpillPID(entry.Name())
		if !ok || pid == os.Getpid() || macWalkPIDAlive(pid) {
			continue
		}
		path := filepath.Join(os.TempDir(), entry.Name())
		if info, err := entry.Info(); err == nil {
			bytes += info.Size()
		}
		if os.Remove(path) == nil {
			reclaimed++
		}
	}
	if reclaimed > 0 {
		fmt.Printf("回收死进程溢写文件 %d 个，释放 %.1f MB\n", reclaimed, float64(bytes)/1024/1024)
	}
}

func parseMacWalkSpillPID(name string) (int, bool) {
	rest, ok := strings.CutPrefix(name, "qc-spill-")
	if !ok || !strings.HasSuffix(name, ".bin") {
		return 0, false
	}
	pidText, _, ok := strings.Cut(rest, "-")
	if !ok {
		return 0, false
	}
	var pid int
	for _, c := range pidText {
		if c < '0' || c > '9' {
			return 0, false
		}
		pid = pid*10 + int(c-'0')
	}
	return pid, pidText != ""
}

// kill(pid, 0) answers reachability. EPERM means another user's live process.
func macWalkPIDAlive(pid int) bool {
	err := syscall.Kill(pid, 0)
	return err == nil || errors.Is(err, syscall.EPERM)
}

func (s *macWalkSpill) create() error {
	reclaimStaleMacWalkSpill()
	path := filepath.Join(os.TempDir(), fmt.Sprintf("qc-spill-%d-%d.bin", os.Getpid(), macWalkSpillSeq.Add(1)))
	file, err := os.OpenFile(path, os.O_CREATE|os.O_TRUNC|os.O_RDWR, 0600)
	if err != nil {
		return err
	}
	s.file, s.path, s.buf = file, path, make([]byte, 0, 1<<20)
	return nil
}

func (s *macWalkSpill) appendEntry(entry MacWalkEntry) error {
	name := []byte(entry.Name)
	if len(name) > 0xffff {
		name = name[:0xffff]
	}
	binary.LittleEndian.PutUint32(s.header[0:4], uint32(entry.Parent))
	s.header[4] = 0
	if entry.IsDir {
		s.header[4] = 1
	}
	mtime := entry.Mtime
	if mtime > 0xffffffff {
		mtime = 0xffffffff
	}
	binary.LittleEndian.PutUint32(s.header[5:9], uint32(mtime))
	binary.LittleEndian.PutUint64(s.header[9:17], entry.Size)
	binary.LittleEndian.PutUint16(s.header[17:19], uint16(len(name)))
	s.buf = append(append(s.buf, s.header[:]...), name...)
	s.n++
	s.nameBytes += 2 + uint64(len(name))
	if len(s.buf) >= cap(s.buf)/2 {
		return s.flush()
	}
	return nil
}

func (s *macWalkSpill) flush() error {
	if len(s.buf) == 0 {
		return nil
	}
	_, err := s.file.Write(s.buf)
	s.buf = s.buf[:0]
	return err
}

func (s *macWalkSpill) finish() error {
	if err := s.flush(); err != nil {
		s.remove()
		return err
	}
	if err := s.file.Sync(); err != nil {
		s.remove()
		return err
	}
	return nil
}

func (s *macWalkSpill) remove() {
	if s.file != nil {
		s.file.Close()
		s.file = nil
	}
	if s.path != "" {
		os.Remove(s.path)
	}
}

// Read returns the next record. io.EOF marks a clean end; a short record is corrupt.
func (s *macWalkSpill) Read() (MacWalkEntry, error) {
	var header [macWalkSpillHeader]byte
	if _, err := io.ReadFull(s.file, header[:]); err != nil {
		return MacWalkEntry{}, err
	}
	n := int(binary.LittleEndian.Uint16(header[17:19]))
	name := make([]byte, n)
	if _, err := io.ReadFull(s.file, name); err != nil {
		if err == io.EOF || err == io.ErrUnexpectedEOF {
			return MacWalkEntry{}, io.ErrUnexpectedEOF
		}
		return MacWalkEntry{}, err
	}
	return MacWalkEntry{
		Parent: uint64(binary.LittleEndian.Uint32(header[0:4])),
		IsDir:  header[4] != 0,
		Mtime:  uint64(binary.LittleEndian.Uint32(header[5:9])),
		Size:   binary.LittleEndian.Uint64(header[9:17]),
		Name:   string(name),
		Used:   true,
	}, nil
}
