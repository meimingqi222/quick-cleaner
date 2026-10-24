package platform

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf16"

	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

const WingetAdminContextProhibited uint32 = 0x8A15007D

type AppRunningProcess struct {
	PID         uint32
	Name, Image string
}

type appTokenCommand struct {
	application, line, cwd, staging string
}

func (c *appTokenCommand) cleanup() {
	if c.staging != "" {
		for _, name := range []string{"launch.ps1", "launch.ps1.json"} {
			_ = os.Remove(filepath.Join(c.staging, name))
		}
		_ = os.Remove(c.staging)
	}
}

func appNativeCommandValid(application, line string) bool {
	return application != "" && !strings.ContainsRune(application, 0) && !strings.ContainsRune(line, 0) && len(utf16.Encode([]rune(line))) < 32767
}

// Arguments remain an already-quoted data string, never generated script source.
func prepareAppTokenCommand(application, line, cwd, cache, powershell string, rejectLinks func(string) error) (_ *appTokenCommand, err error) {
	if !appNativeCommandValid(application, line) {
		return nil, fmt.Errorf("invalid native command line")
	}
	c := &appTokenCommand{application: application, line: line, cwd: cwd}
	if len(utf16.Encode([]rune(line))) < 1024 {
		return c, nil
	}
	if cache == "" || powershell == "" {
		return nil, fmt.Errorf("missing desktop-user command staging directory or PowerShell")
	}
	tail, ok := strings.CutPrefix(line, QuoteWindowsArgument(application))
	if !ok || (tail != "" && !strings.HasPrefix(tail, " ")) {
		return nil, fmt.Errorf("invalid structured command line")
	}
	root := filepath.Join(cache, "QuickCleaner", "command-staging")
	if err = rejectLinks(root); err != nil {
		return nil, err
	}
	if err = os.MkdirAll(root, 0700); err != nil {
		return nil, err
	}
	if err = rejectLinks(root); err != nil {
		return nil, err
	}
	c.staging, err = os.MkdirTemp(root, "launch-")
	if err != nil {
		return nil, err
	}
	defer func() {
		if err != nil {
			c.cleanup()
		}
	}()
	c.application = powershell
	c.line = appExecutableLine(powershell, []string{"-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", filepath.Join(c.staging, "launch.ps1")})
	if len(utf16.Encode([]rune(c.line))) >= 1024 {
		return nil, fmt.Errorf("desktop-user dispatcher path exceeds native command limit")
	}
	payload, err := json.Marshal(struct {
		Application string `json:"application"`
		Arguments   string `json:"arguments"`
		CWD         string `json:"cwd"`
	}{application, strings.TrimLeft(tail, " "), cwd})
	if err != nil {
		return nil, err
	}
	for _, file := range []struct {
		name string
		data []byte
	}{{"launch.ps1.json", payload}, {"launch.ps1", []byte(appTokenDispatcher)}} {
		f, e := os.OpenFile(filepath.Join(c.staging, file.name), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if e != nil {
			return nil, e
		}
		_, e = f.Write(file.data)
		closeErr := f.Close()
		if e != nil {
			return nil, e
		}
		if closeErr != nil {
			return nil, closeErr
		}
	}
	return c, nil
}

func appExecutableLine(application string, args []string) string {
	quoted := []string{QuoteWindowsArgument(application)}
	for _, arg := range args {
		quoted = append(quoted, QuoteWindowsArgument(arg))
	}
	return strings.Join(quoted, " ")
}

func appRelatedProcess(image, name, installDir, stem string) bool {
	lower := cases.Lower(language.Und)
	normalize := func(s string) string { return lower.String(strings.ReplaceAll(s, "/", `\`)) }
	image, name, installDir, stem = normalize(image), normalize(name), normalize(installDir), lower.String(stem)
	if installDir != "" && strings.HasPrefix(image, strings.TrimRight(installDir, `\`)+`\`) {
		return true
	}
	if stem == "" {
		return false
	}
	if i := strings.LastIndexByte(name, '.'); i >= 0 {
		name = name[:i]
	}
	return name == stem
}

const appTokenDispatcher = `$ErrorActionPreference = 'Stop'
try {
    $payload = Get-Content -LiteralPath ($PSCommandPath + '.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = [string]$payload.application
    $info.Arguments = [string]$payload.arguments
    if ($payload.cwd) { $info.WorkingDirectory = [string]$payload.cwd }
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $process = [System.Diagnostics.Process]::Start($info)
    $process.WaitForExit()
    $code = $process.ExitCode
    $process.Dispose()
    exit $code
} catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }
`
