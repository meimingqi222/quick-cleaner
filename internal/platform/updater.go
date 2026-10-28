package platform

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

func LooksLikeDevBuild(exe string) bool {
	s := strings.ToLower(exe)
	return strings.Contains(s, `target\debug`) || strings.Contains(s, "target/debug") ||
		strings.Contains(s, `target\release`) || strings.Contains(s, "target/release")
}

// UpdateBundleForExecutable finds the nearest bundle, including nested helpers.
// TCC's fixed executable-depth lookup deliberately has a different contract.
func UpdateBundleForExecutable(exe string) (string, bool) {
	for p := exe; ; p = filepath.Dir(p) {
		if filepath.Ext(p) == ".app" {
			return p, true
		}
		if parent := filepath.Dir(p); parent == p || p == "." {
			return "", false
		}
	}
}

func UpdateCacheDir() (string, bool) {
	root, ok := UserDataDir()
	if !ok {
		return "", false
	}
	return filepath.Join(root, "QuickCleaner", "update-cache"), true
}

func updateWriteProbe(target string) error {
	parent := filepath.Dir(target)
	probe := filepath.Join(parent, fmt.Sprintf(".qc-update-write-probe-%d", os.Getpid()))
	if err := os.WriteFile(probe, []byte("ok"), 0600); err != nil {
		return fmt.Errorf("install directory is not writable (%s): %w", parent, err)
	}
	_ = os.Remove(probe)
	return nil
}

func prepareUpdateCache() (string, error) {
	cache, ok := UpdateCacheDir()
	if !ok {
		return "", fmt.Errorf("user data dir unavailable")
	}
	if err := os.MkdirAll(cache, 0700); err != nil {
		return "", fmt.Errorf("create cache: %w", err)
	}
	return cache, nil
}

// RemoveUpdateBackup never removes a directory as an exe backup, or follows a
// linked backup. Startup cleanup is best effort, as in the reference installer.
func RemoveUpdateBackup(target string, bundle bool) {
	old := target + ".old"
	info, err := os.Lstat(old)
	if err != nil || info.Mode()&os.ModeSymlink != 0 {
		return
	}
	if bundle && info.IsDir() {
		_ = os.RemoveAll(old)
	} else if !bundle && info.Mode().IsRegular() {
		_ = os.Remove(old)
	}
}

func WindowsReplaceScript(current, payload, log string, parentPID int) string {
	quote := func(s string) string { return "'" + strings.ReplaceAll(s, "'", "''") + "'" }
	return fmt.Sprintf(`$ErrorActionPreference = 'Stop'
$exe = %s
$new = %s
$old = "$exe.old"
$log = %s
function Log($m) { Add-Content -Path $log -Value ("$(Get-Date -Format o) " + $m) }
try {
  Log "waiting for pid %d"
  Wait-Process -Id %d -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 400
  if (Test-Path -LiteralPath $old) { Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue }
  Move-Item -LiteralPath $exe -Destination $old -Force
  try {
    Copy-Item -LiteralPath $new -Destination $exe -Force
    Log "copied new binary"
    Start-Process -FilePath $exe
    Log "relaunched"
  } catch {
    Log ("COPY FAILED, restoring: " + $_.Exception.Message)
    if (Test-Path -LiteralPath $exe) { Remove-Item -LiteralPath $exe -Force -ErrorAction SilentlyContinue }
    if (Test-Path -LiteralPath $old) {
      Move-Item -LiteralPath $old -Destination $exe -Force
      Start-Process -FilePath $exe
      Log "restored previous binary and relaunched"
    } else {
      Log "CRITICAL: no .old to restore"
    }
    exit 1
  }
} catch {
  Log ("FAILED: " + $_.Exception.Message)
  exit 1
}
`, quote(current), quote(payload), quote(log), parentPID, parentPID)
}

func MacosBundleReplaceScript(current, payload, log string, parentPID int) string {
	quote := func(s string) string { return "'" + strings.ReplaceAll(s, "'", "'\\''") + "'" }
	return fmt.Sprintf(`#!/bin/bash
set -euo pipefail
trap '' HUP
CUR=%s
NEW=%s
OLD="${CUR}.old"
LOG=%s
log() { echo "$(date -Iseconds) $*" >> "$LOG"; }
log "waiting for pid %d"
while kill -0 %d 2>/dev/null; do sleep 0.2; done
sleep 0.3
rm -rf "$OLD"
mv "$CUR" "$OLD"
if cp -R "$NEW" "$CUR"; then
  log "replaced app bundle"
  open "$CUR"
  log "relaunched"
else
  log "COPY FAILED, restoring previous bundle"
  rm -rf "$CUR"
  mv "$OLD" "$CUR"
  open "$CUR"
  log "restored and relaunched previous bundle"
  exit 1
fi
`, quote(current), quote(payload), quote(log), parentPID, parentPID)
}
