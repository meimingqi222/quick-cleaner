package core

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"
)

// UpdateHTTPClient keeps network configuration injectable without changing the
// release/checksum discovery policy. Configure before sharing between goroutines.
// A nil Client uses the standard transport and the upstream five-redirect limit.
type UpdateHTTPClient struct {
	Client           *http.Client
	UserAgent        string
	LatestReleaseURL string
}

func NewUpdateHTTPClient(appVersion string) *UpdateHTTPClient {
	return &UpdateHTTPClient{UserAgent: "QuickCleaner-Updater/" + appVersion, LatestReleaseURL: "https://api.github.com/repos/" + GithubOwner + "/" + GithubRepo + "/releases/latest"}
}

func (c *UpdateHTTPClient) get(ctx context.Context, url, accept string) (*http.Response, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", c.UserAgent)
	if accept != "" {
		req.Header.Set("Accept", accept)
	}
	client := c.Client
	if client == nil {
		client = &http.Client{CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) > 5 {
				return fmt.Errorf("too many redirects")
			}
			return nil
		}}
	}
	resp, err := client.Do(req)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode >= 400 {
		_ = resp.Body.Close()
		return nil, fmt.Errorf("%s: status code %d", url, resp.StatusCode)
	}
	return resp, nil
}

func (c *UpdateHTTPClient) FetchLatestRelease(ctx context.Context, timeout time.Duration) (GithubRelease, error) {
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	resp, err := c.get(ctx, c.LatestReleaseURL, "application/vnd.github+json")
	if err != nil {
		return GithubRelease{}, fmt.Errorf("github api: %w", err)
	}
	defer resp.Body.Close()
	// serde_json::from_reader requires end-of-input, not just the first value.
	b, err := io.ReadAll(resp.Body)
	if err != nil {
		return GithubRelease{}, fmt.Errorf("decode release json: %w", err)
	}
	var release GithubRelease
	if err = json.Unmarshal(b, &release); err != nil {
		return GithubRelease{}, fmt.Errorf("decode release json: %w", err)
	}
	return release, nil
}

func (c *UpdateHTTPClient) FetchText(ctx context.Context, url string, timeout time.Duration) (string, error) {
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	resp, err := c.get(ctx, url, "")
	if err != nil {
		return "", fmt.Errorf("fetch: %w", err)
	}
	defer resp.Body.Close()
	// ureq 2.12.1 into_string has this limit; JSON and file downloads do not.
	const limit = 10 * 1024 * 1024
	b, err := io.ReadAll(io.LimitReader(resp.Body, limit+1))
	if err != nil {
		return "", fmt.Errorf("read body: %w", err)
	}
	if len(b) > limit {
		return "", fmt.Errorf("read body: response too big for into_string")
	}
	return strings.ToValidUTF8(string(b), "\ufffd"), nil
}

// DownloadToFile retains partial downloads on failure, as the Rust caller does.
// Progress runs synchronously after each successful write, with no synthetic
// initial/final events and nil total when Content-Length is unknown.
func (c *UpdateHTTPClient) DownloadToFile(ctx context.Context, url, dest string, onProgress func(uint64, *uint64)) error {
	if err := os.MkdirAll(filepath.Dir(dest), 0777); err != nil {
		return fmt.Errorf("create download dir: %w", err)
	}
	ctx, cancel := context.WithTimeout(ctx, 300*time.Second)
	defer cancel()
	resp, err := c.get(ctx, url, "")
	if err != nil {
		return fmt.Errorf("download: %w", err)
	}
	defer resp.Body.Close()
	var total *uint64
	if h := resp.Header.Get("Content-Length"); h != "" {
		if n, e := strconv.ParseUint(h, 10, 64); e == nil {
			total = &n
		}
	}
	f, err := os.Create(dest)
	if err != nil {
		return fmt.Errorf("create %s: %w", dest, err)
	}
	defer f.Close()
	buf := make([]byte, 64*1024)
	var transferred uint64
	for {
		n, e := resp.Body.Read(buf)
		if n > 0 {
			if err = ctx.Err(); err != nil {
				return fmt.Errorf("read download: %w", err)
			}
			if _, err = f.Write(buf[:n]); err != nil {
				return fmt.Errorf("write download: %w", err)
			}
			transferred += uint64(n)
			if onProgress != nil {
				var value *uint64
				if total != nil {
					copy := *total
					value = &copy
				}
				onProgress(transferred, value)
			}
		}
		if e != nil {
			if e == io.EOF {
				return nil
			}
			return fmt.Errorf("read download: %w", e)
		}
	}
}

// These convenience functions match the application-level Rust entry points.
func FetchLatestRelease(ctx context.Context, timeout time.Duration) (GithubRelease, error) {
	return NewUpdateHTTPClient("0.0.16").FetchLatestRelease(ctx, timeout)
}
func FetchText(ctx context.Context, url string, timeout time.Duration) (string, error) {
	return NewUpdateHTTPClient("0.0.16").FetchText(ctx, url, timeout)
}
func DownloadToFile(ctx context.Context, url, dest string, onProgress func(uint64, *uint64)) error {
	return NewUpdateHTTPClient("0.0.16").DownloadToFile(ctx, url, dest, onProgress)
}

// serde's required fields reject missing/null fields and known duplicates;
// encoding/json's default zero-value/last-key behavior would hide bad releases.
func updaterJSONFields(data []byte, required, optional []string) (map[string]json.RawMessage, error) {
	if !utf8.Valid(data) {
		return nil, fmt.Errorf("invalid UTF-8")
	}
	known := map[string]bool{}
	for _, k := range required {
		known[k] = true
	}
	for _, k := range optional {
		known[k] = true
	}
	d := json.NewDecoder(bytes.NewReader(data))
	token, err := d.Token()
	if err != nil {
		return nil, err
	}
	if token != json.Delim('{') {
		return nil, fmt.Errorf("expected release object")
	}
	fields := map[string]json.RawMessage{}
	for d.More() {
		t, err := d.Token()
		if err != nil {
			return nil, err
		}
		key := t.(string)
		var raw json.RawMessage
		if err = d.Decode(&raw); err != nil {
			return nil, err
		}
		if known[key] {
			if _, ok := fields[key]; ok {
				return nil, fmt.Errorf("duplicate field %q", key)
			}
			fields[key] = raw
		}
	}
	if _, err = d.Token(); err != nil {
		return nil, err
	}
	if _, err = d.Token(); err != io.EOF {
		return nil, fmt.Errorf("trailing data after release")
	}
	for _, k := range required {
		if raw, ok := fields[k]; !ok || bytes.Equal(bytes.TrimSpace(raw), []byte("null")) {
			return nil, fmt.Errorf("missing or null field %q", k)
		}
	}
	return fields, nil
}

func (r *GithubRelease) UnmarshalJSON(data []byte) error {
	fields, err := updaterJSONFields(data, []string{"tag_name", "html_url", "prerelease", "draft", "assets"}, []string{"name", "body"})
	if err != nil {
		return err
	}
	// Decode only exact known keys; Go otherwise matches unknown case variants.
	data, err = json.Marshal(fields)
	if err != nil {
		return err
	}
	type plain GithubRelease
	var value plain
	if err = json.Unmarshal(data, &value); err != nil {
		return err
	}
	*r = GithubRelease(value)
	return nil
}

func (a *GithubAsset) UnmarshalJSON(data []byte) error {
	fields, err := updaterJSONFields(data, []string{"name", "browser_download_url", "size"}, nil)
	if err != nil {
		return err
	}
	data, err = json.Marshal(fields)
	if err != nil {
		return err
	}
	type plain GithubAsset
	var value plain
	if err = json.Unmarshal(data, &value); err != nil {
		return err
	}
	*a = GithubAsset(value)
	return nil
}
