package platform

import (
	"encoding/binary"
	"strings"
	"unicode"

	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

// AppActivityIndex indexes native execution evidence, not uninstall authority.
// Its zero value is ready to use. Populate before sharing it with readers.
type AppActivityIndex struct{ byToken map[string]uint64 }

type appTimestampMap map[string]uint64

// Insert records a lower-case path, filename, stem and directory segments.
// Like Rust's UserAssistIndex, inputs and lookups are not case-normalized here.
func (i *AppActivityIndex) Insert(path string, ts uint64) {
	if i.byToken == nil {
		i.byToken = make(
			map[string]uint64,
		)
	}
	record := func(key string) { i.byToken[key] = max(i.byToken[key], ts) }
	record(path)
	parts := strings.FieldsFunc(path, func(r rune) bool { return r == '\\' || r == '/' })
	if len(parts) > 0 {
		file := parts[len(parts)-1]
		record(file)
		stem := file
		if dot := strings.LastIndexByte(file, '.'); dot > 0 {
			stem = file[:dot]
		}
		record(stem)
	}
	for _, seg := range parts {
		if len(seg) >= 3 {
			record(seg)
		}
	}
}

func (i AppActivityIndex) LookupOne(token string) uint64 { return i.byToken[token] }
func (i AppActivityIndex) Lookup(tokens []string) uint64 {
	var ts uint64
	for _, token := range tokens {
		ts = max(ts, i.LookupOne(token))
	}
	return ts
}

func appFiletimeUnix(ft uint64) uint64 {
	const epoch = 116444736000000000
	if ft <= epoch {
		return 0
	}
	ts := (ft - epoch) / 10000000
	if ts < 946684800 || ts >= 4102444800 {
		return 0
	}
	return ts
}

func appRunBlobUnix(data []byte) uint64 {
	offset := 0
	switch {
	case len(data) >= 68:
		offset = 60
	case len(data) >= 16:
		offset = 8
	case len(data) < 8:
		return 0
	}
	return appFiletimeUnix(binary.LittleEndian.Uint64(data[offset : offset+8]))
}

func appROT13(s string) string {
	return strings.Map(func(r rune) rune {
		switch {
		case r >= 'a' && r <= 'z':
			return 'a' + (r-'a'+13)%26
		case r >= 'A' && r <= 'Z':
			return 'A' + (r-'A'+13)%26
		}
		return r
	}, s)
}

func appUserAssistName(raw string) string {
	s := strings.ToLower(appROT13(raw))
	if pos := strings.IndexByte(s, ':'); pos > 0 {
		previous := []rune(s[:pos])
		c := previous[len(previous)-1]
		if !(c >= 'a' && c <= 'z') {
			s = s[pos+1:]
		}
	}
	return s
}

func appNormalizeNTPath(raw string, dos map[string]string) string {
	s := strings.ToLower(raw)
	for _, prefix := range []string{`\??\`, `\\?\`} {
		if strings.HasPrefix(s, prefix) {
			s = s[len(prefix):]
			break
		}
	}
	best, result := 0, s
	for dev, drive := range dos {
		if strings.HasPrefix(s, dev) {
			rest := s[len(dev):]
			if (rest == "" || strings.HasPrefix(rest, `\`)) && len(dev) > best {
				best, result = len(dev), drive+rest
			}
		}
	}
	return result
}

func appPrefetchExe(filename string) string {
	s := strings.ToLower(filename)
	if !strings.HasSuffix(s, ".pf") {
		return ""
	}
	s = strings.TrimSuffix(s, ".pf")
	dash := strings.LastIndexByte(s, '-')
	if dash <= 0 || len(s)-dash-1 != 8 {
		return ""
	}
	for _, c := range s[dash+1:] {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
			return ""
		}
	}
	return s[:dash]
}

// ExtractAppTokens matches the Rust name tokenizer, including its UTF-8 byte
// length threshold and retention of non-alphanumeric characters above U+4E00.
func ExtractAppTokens(name string) []string {
	var out []string
	for _, part := range strings.FieldsFunc(cases.Lower(language.Und).String(name), func(r rune) bool {
		return r < '\u4e00' && !unicode.IsLetter(r) && !unicode.IsNumber(r) && !unicode.Is(unicode.Properties["Other_Alphabetic"], r)
	}) {
		part = strings.TrimSpace(part)
		if len(part) < 3 {
			continue
		}
		switch part {
		case "64bit", "32bit", "x64", "x86", "bit", "setup", "installer", "version", "microsoft", "corporation", "windows":
			continue
		}
		out = append(out, part)
	}
	return out
}
