package core

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"
)

type HistoryEntry struct {
	Timestamp  string   `json:"ts"`
	Action     string   `json:"action"`
	Targets    []string `json:"targets"`
	OK         uint     `json:"ok"`
	Skipped    uint     `json:"skipped"`
	Failed     uint     `json:"failed"`
	FreedBytes uint64   `json:"freed_bytes"`
}

// RecordHistory shares the trusted user's settings directory. Failures are
// best effort and cannot turn a completed cleanup into a failed action.
func RecordHistory(action string, targets []string, ok, skipped, failed uint, freedBytes uint64) {
	dir, available := configDir()
	if !available || os.MkdirAll(dir, 0777) != nil {
		return
	}
	entry := HistoryEntry{
		Timestamp: time.Now().Format("2006-01-02T15:04:05-07:00"), Action: action,
		Targets: make([]string, len(targets)), OK: ok, Skipped: skipped, Failed: failed, FreedBytes: freedBytes,
	}
	for i, target := range targets {
		entry.Targets[i] = strings.ToValidUTF8(target, "\uFFFD")
	}
	line, err := json.Marshal(entry)
	if err != nil {
		return
	}
	path := filepath.Join(dir, "history.jsonl")
	content, err := os.ReadFile(path)
	if err != nil || !utf8.Valid(content) {
		content = nil
	}
	// Check the existing length before appending, matching the Rust threshold.
	if len(content) > 1024*1024 {
		start := len(content) - 512*1024
		if newline := bytes.IndexByte(content[start:], '\n'); newline >= 0 {
			start += newline + 1
		}
		content = content[start:]
	}
	content = append(content, line...)
	content = append(content, '\n')
	_ = os.WriteFile(path, content, 0666)
}
