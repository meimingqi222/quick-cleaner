package core

import (
	"fmt"
	"log"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

const (
	logFileName = "quick-cleaner.log"
	logOldName  = "quick-cleaner.log.1"
	logMaxBytes = 2 * 1024 * 1024
)

var (
	logMu   sync.Mutex
	logFile *os.File
)

// InitProductLog opens the original-user log next to settings and rotates it
// once when the previous file is larger than 2MB. Failure is ignored: a
// cleaner must still run when the log cannot be written.
func InitProductLog(version string, elevated bool, pid int) {
	dir, ok := configDir()
	file := (*os.File)(nil)
	if ok {
		file = openProductLog(dir)
	}
	logMu.Lock()
	if logFile != nil && logFile != file {
		_ = logFile.Close()
	}
	logFile = file
	logMu.Unlock()
	if file != nil {
		log.SetFlags(0)
		log.SetOutput(productLog{})
	}
	state := "否"
	if elevated {
		state = "是"
	}
	log.Printf("===== QuickCleaner v%s 启动 | 提权=%s | pid=%d =====", version, state, pid)
}

// LogPanic records a recovered panic. It never panics.
func LogPanic(where string, recovered any, stack []byte) {
	defer func() { _ = recover() }()
	log.Printf("!!!!! panic 于 %s: %v\n调用栈:\n%s", where, recovered, stack)
}

type productLog struct{}

func (productLog) Write(p []byte) (int, error) {
	text := strings.TrimRight(string(p), "\r\n")
	if text == "" {
		return len(p), nil
	}
	line := fmt.Sprintf("[%s] %s\n", time.Now().Format("2006-01-02 15:04:05.000"), text)
	logMu.Lock()
	defer logMu.Unlock()
	if logFile == nil {
		return len(p), nil
	}
	_, _ = logFile.WriteString(line)
	_ = logFile.Sync()
	return len(p), nil
}

func openProductLog(dir string) *os.File {
	if dir == "" {
		return nil
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return nil
	}
	for _, name := range []string{logFileName, logOldName, "history.jsonl", "settings.json"} {
		restrictExistingFile(filepath.Join(dir, name))
	}
	path := filepath.Join(dir, logFileName)
	if info, err := os.Stat(path); err == nil && info.Size() > logMaxBytes {
		old := filepath.Join(dir, logOldName)
		_ = os.Remove(old)
		_ = os.Rename(path, old)
	}
	file, err := os.OpenFile(path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		return nil
	}
	if file.Chmod(0600) != nil {
		_ = file.Close()
		return nil
	}
	return file
}
