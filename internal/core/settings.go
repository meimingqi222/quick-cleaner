package core

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// JSON names and defaults are the Rust release's on-disk compatibility contract.
type Settings struct {
	Language             Language `json:"language"`
	DeleteToRecycleBin   bool     `json:"delete_to_recycle_bin"`
	MacOSFDADismissed    bool     `json:"macos_fda_dismissed"`
	Whitelist            []string `json:"whitelist"`
	BrewCleanupAt        *int64   `json:"brew_cleanup_at"`
	AutoCheckUpdates     bool     `json:"auto_check_updates"`
	SkippedUpdateVersion *string  `json:"skipped_update_version"`
	LastUpdateCheckAt    *int64   `json:"last_update_check_at"`
}

func DefaultSettings() Settings {
	return Settings{
		Language:  LanguageFromLocaleTag(platform.SystemLanguageTag()),
		Whitelist: []string{}, AutoCheckUpdates: true,
	}
}

// UnmarshalJSON supplies missing-field defaults without accepting null or
// duplicate required fields that serde rejects. Unknown fields remain ignored.
func (s *Settings) UnmarshalJSON(data []byte) error {
	d := json.NewDecoder(bytes.NewReader(data))
	if token, err := d.Token(); err != nil || token != json.Delim('{') {
		return fmt.Errorf("settings must be an object")
	}
	next := DefaultSettings()
	seen := make(map[string]bool)
	for d.More() {
		token, err := d.Token()
		if err != nil {
			return err
		}
		key := token.(string)
		var raw json.RawMessage
		if err := d.Decode(&raw); err != nil {
			return err
		}
		var field any
		var paths []*string
		optional := false
		switch key {
		case "language":
			field = &next.Language
		case "delete_to_recycle_bin":
			field = &next.DeleteToRecycleBin
		case "macos_fda_dismissed":
			field = &next.MacOSFDADismissed
		case "whitelist":
			field = &paths
		case "brew_cleanup_at":
			field, optional = &next.BrewCleanupAt, true
		case "auto_check_updates":
			field = &next.AutoCheckUpdates
		case "skipped_update_version":
			field, optional = &next.SkippedUpdateVersion, true
		case "last_update_check_at":
			field, optional = &next.LastUpdateCheckAt, true
		default:
			continue
		}
		if seen[key] {
			return fmt.Errorf("duplicate settings field %q", key)
		}
		seen[key] = true
		if !optional && bytes.Equal(raw, []byte("null")) {
			return fmt.Errorf("null settings field %q", key)
		}
		if err := json.Unmarshal(raw, field); err != nil {
			return err
		}
		if key == "whitelist" {
			next.Whitelist = make([]string, len(paths))
			for i, path := range paths {
				if path == nil {
					return fmt.Errorf("null whitelist entry")
				}
				next.Whitelist[i] = *path
			}
		}
	}
	if _, err := d.Token(); err != nil {
		return err
	}
	*s = next
	return nil
}

func MergeSettingsJSON(text string) Settings {
	var s Settings
	if json.Unmarshal([]byte(text), &s) != nil {
		return DefaultSettings()
	}
	return s
}

func configDir() (string, bool) {
	dir, ok := platform.UserDataDir()
	if !ok {
		return "", false
	}
	return filepath.Join(dir, "QuickCleaner"), true
}

func SettingsPath() (string, bool) {
	dir, ok := configDir()
	if !ok {
		return "", false
	}
	return filepath.Join(dir, "settings.json"), true
}

func LoadSettings() Settings {
	path, ok := SettingsPath()
	if !ok {
		return DefaultSettings()
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return DefaultSettings()
	}
	s := MergeSettingsJSON(string(data))
	if !ReloadWhitelist(s.Whitelist) {
		fmt.Fprintln(os.Stderr, "QuickCleaner: whitelist was only partially loaded")
	}
	return s
}

// Save is best effort, as in the release: a persistence failure does not
// interrupt the user's action. The temporary file is on the destination volume.
func (s Settings) Save() {
	path, ok := SettingsPath()
	if !ok || os.MkdirAll(filepath.Dir(path), 0777) != nil {
		return
	}
	data, err := json.MarshalIndent(s, "", "  ")
	if err != nil {
		return
	}
	tmp := path + ".tmp"
	if writePrivateFile(tmp, data) != nil {
		return
	}
	if os.Rename(tmp, path) != nil {
		_ = os.Remove(path)
		if os.Rename(tmp, path) != nil {
			_ = os.Remove(tmp)
			_ = writePrivateFile(path, data)
		}
	}
}
