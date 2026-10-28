package core

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"runtime"
	"strconv"
	"strings"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

const (
	GithubOwner              = "meimingqi222"
	GithubRepo               = "quick-cleaner"
	FirstCheckDelayMS uint64 = 10_000
	CheckIntervalMS   uint64 = 4 * 60 * 60 * 1000
)

type DownloadProgress struct {
	Percent     float32
	Transferred uint64
	Total       *uint64
}

type UpdateOperation uint8

const (
	UpdateCheck UpdateOperation = iota
	UpdateDownload
	UpdateInstall
)

type UpdateStatusKind uint8

const (
	UpdateIdle UpdateStatusKind = iota
	UpdateChecking
	UpdateNotAvailable
	UpdateAvailable
	UpdateDownloading
	UpdateVerifying
	UpdateDownloaded
	UpdateInstalling
	UpdateError
)

// Only fields belonging to Kind are meaningful, matching the Rust enum variants.
type UpdateStatus struct {
	Kind           UpdateStatusKind
	CurrentVersion string
	LatestVersion  string
	ReleaseURL     string
	Notes          string
	AssetName      string
	AssetURL       string
	ChecksumURL    string
	Progress       DownloadProgress
	Payload        string
	Operation      UpdateOperation
	Message        string
}

func (s UpdateStatus) LatestVersionValue() *string {
	switch s.Kind {
	case UpdateAvailable, UpdateDownloading, UpdateVerifying, UpdateDownloaded, UpdateInstalling:
		v := s.LatestVersion
		return &v
	}
	return nil
}

func (s UpdateStatus) WantsAttention() bool {
	switch s.Kind {
	case UpdateAvailable, UpdateDownloading, UpdateVerifying, UpdateDownloaded, UpdateInstalling, UpdateError:
		return true
	}
	return false
}

type GithubRelease struct {
	TagName    string        `json:"tag_name"`
	HTMLURL    string        `json:"html_url"`
	Name       *string       `json:"name"`
	Body       *string       `json:"body"`
	Prerelease bool          `json:"prerelease"`
	Draft      bool          `json:"draft"`
	Assets     []GithubAsset `json:"assets"`
}

type GithubAsset struct {
	Name               string `json:"name"`
	BrowserDownloadURL string `json:"browser_download_url"`
	Size               uint64 `json:"size"`
}

func NormalizeVersion(v string) string { return strings.TrimLeft(strings.TrimSpace(v), "vV") }

func updaterVersionParts(v string) ([3]uint64, bool) {
	var parts [3]uint64
	s := strings.Split(NormalizeVersion(v), ".")
	parse := func(s string) (uint64, error) {
		// Rust's unsigned parser permits a leading +, but not whitespace.
		n, err := strconv.ParseUint(strings.TrimPrefix(s, "+"), 10, 64)
		if err != nil {
			return 0, err
		}
		return n, nil
	}
	major, err := parse(s[0])
	if err != nil {
		return parts, false
	}
	parts[0] = major
	if len(s) > 1 {
		parts[1], _ = parse(s[1])
	}
	if len(s) > 2 {
		end := 0
		for end < len(s[2]) && s[2][end] >= '0' && s[2][end] <= '9' {
			end++
		}
		parts[2], _ = parse(s[2][:end])
	}
	return parts, true
}

func IsVersionNewer(candidate, current string) bool {
	n, a := updaterVersionParts(candidate)
	b, c := updaterVersionParts(current)
	if !a || !c {
		return NormalizeVersion(candidate) != NormalizeVersion(current)
	}
	for i := range n {
		if n[i] != b[i] {
			return n[i] > b[i]
		}
	}
	return false
}

func IsSkipped(latest string, skipped *string) bool {
	return skipped != nil && NormalizeVersion(latest) == NormalizeVersion(*skipped)
}
func LooksLikeDevBuild(exe string) bool {
	return platform.LooksLikeDevBuild(exe)
}

type UpdateTarget uint8

const (
	WindowsX64 UpdateTarget = iota
	MacosUniversal
	MacosAarch64
	MacosX86_64
)

func CurrentTarget() UpdateTarget {
	if runtime.GOOS == "darwin" {
		if runtime.GOARCH == "arm64" {
			return MacosAarch64
		}
		return MacosX86_64
	}
	return WindowsX64
}

func CandidateAssetNames(target UpdateTarget) []string {
	switch target {
	case MacosUniversal:
		return []string{"quick-cleaner-macos-universal.zip"}
	case MacosAarch64:
		return []string{"quick-cleaner-macos-universal.zip", "quick-cleaner-macos-aarch64.zip"}
	case MacosX86_64:
		return []string{"quick-cleaner-macos-universal.zip", "quick-cleaner-macos-x86_64.zip"}
	default:
		return []string{"quick-cleaner-windows-x86_64.zip"}
	}
}

func SelectAssetByCandidates(release *GithubRelease, candidates []string) *GithubAsset {
	if release.Draft || release.Prerelease {
		return nil
	}
	for _, name := range candidates {
		for i := range release.Assets {
			if release.Assets[i].Name == name {
				return &release.Assets[i]
			}
		}
	}
	return nil
}

func SelectAsset(release *GithubRelease, target UpdateTarget) *GithubAsset {
	names := CandidateAssetNames(target)
	return SelectAssetByCandidates(release, names[len(names)-1:])
}

func EvaluateRelease(current string, release *GithubRelease, candidates []string) (UpdateStatus, error) {
	s := UpdateStatus{Kind: UpdateNotAvailable, CurrentVersion: current}
	if release.Draft || release.Prerelease || !IsVersionNewer(release.TagName, current) {
		return s, nil
	}
	a := SelectAssetByCandidates(release, candidates)
	if a == nil {
		quoted := make([]string, len(candidates))
		for i, n := range candidates {
			quoted[i] = strconv.Quote(n)
		}
		return UpdateStatus{}, fmt.Errorf("no matching asset for this platform among [%s]", strings.Join(quoted, ", "))
	}
	s.Kind = UpdateAvailable
	s.LatestVersion = NormalizeVersion(release.TagName)
	s.ReleaseURL = release.HTMLURL
	if release.Body != nil {
		s.Notes = *release.Body
	}
	s.AssetName = a.Name
	s.AssetURL = a.BrowserDownloadURL
	s.ChecksumURL = s.AssetURL + ".sha256"
	return s, nil
}

func ParseChecksumSidecar(text string) *string {
	text = strings.TrimLeft(text, "\ufeff")
	for _, line := range strings.Split(text, "\n") {
		fields := strings.Fields(line)
		if len(fields) == 0 {
			continue
		}
		h := fields[0]
		if len(h) != 64 {
			return nil
		}
		if _, err := hex.DecodeString(h); err != nil {
			return nil
		}
		h = strings.ToLower(h)
		return &h
	}
	return nil
}

func SHA256File(path string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()
	h := sha256.New()
	if _, err = io.CopyBuffer(h, f, make([]byte, 64*1024)); err != nil {
		return "", err
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}

func VerifyChecksum(path, sidecar string) error {
	expected := ParseChecksumSidecar(sidecar)
	if expected == nil {
		return fmt.Errorf("checksum sidecar is malformed")
	}
	actual, err := SHA256File(path)
	if err != nil {
		return err
	}
	if actual != *expected {
		return fmt.Errorf("checksum mismatch: expected %s, got %s", *expected, actual)
	}
	return nil
}
