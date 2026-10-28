package core

import "testing"

func TestNamePatternPort(t *testing.T) {
	for _, tc := range []struct {
		pattern, name string
		want          bool
	}{
		{"", "anything", false}, {"report", "Annual_REPORT_final.pdf", true}, {"REPORT", "summary", false},
		{"*.TXT", "README.txt", true}, {"*.txt", "notes.txt.bak", false}, {"a?c", "a中c", true},
		{"a?c", "ac", false}, {"a?c", "abbc", false}, {"???", "中🙂a", true}, {"??", "中🙂a", false},
		{"*", "", true}, {"**abc**", "xyzabcxyz", true}, {"a**b**c", "aXXbYYc", true},
		{"*report*.pdf", "annual_report_final.pdf", true}, {"*report*.pdf", "annual_summary.pdf", false},
		{"*.mp4", "mp4_readme.txt", false}, {"Ä", "ä", false}, {"Ä", "Ä", true},
	} {
		if got := ParseNamePattern(tc.pattern).MatchesRaw(tc.name); got != tc.want {
			t.Errorf("%q on %q = %v", tc.pattern, tc.name, got)
		}
	}
}

func TestIgnoredNamesAndInjectablePhotoRules(t *testing.T) {
	for _, name := range []string{"$RECYCLE.BIN", "System Volume Information", "node_modules", ".git", "Library", "__pycache__", "DerivedData"} {
		if !IsIgnoredDirName(name) || !IsDeclutterIgnoredDirName(name) || !IsPhotoIgnoredDirName(name, nil) {
			t.Fatal(name)
		}
	}
	rules := func(id, key string) []string {
		if id != "engine" || key != "photo_ignored_directories" {
			t.Fatal(id, key)
		}
		return []string{"site", "help", "manuals"}
	}
	for _, name := range []string{"SITE", "help", "manuals"} {
		if !IsPhotoIgnoredDirName(name, rules) || IsDeclutterIgnoredDirName(name) {
			t.Fatal(name)
		}
	}
	for _, name := range []string{"Pictures", "我的照片", "Projects"} {
		if IsPhotoIgnoredDirName(name, rules) {
			t.Fatal(name)
		}
	}
}
