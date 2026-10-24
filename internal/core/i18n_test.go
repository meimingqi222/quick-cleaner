package core_test

import (
	"encoding/json"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func TestLanguages(t *testing.T) {
	var zero core.Language
	if zero != core.Chinese || core.English != 1 {
		t.Fatal("language storage contract")
	}
	for _, c := range []struct {
		l                 core.Language
		code, name, short string
		other             core.Language
	}{{core.Chinese, "zh", "中文", "中", core.English}, {core.English, "en", "English", "EN", core.Chinese}} {
		if c.l.Code() != c.code || c.l.DisplayName() != c.name || c.l.ShortName() != c.short || c.l.Toggle() != c.other {
			t.Errorf("metadata: %v", c.l)
		}
	}
	if core.AllLanguages() != [2]core.Language{core.Chinese, core.English} {
		t.Fatal("all languages")
	}
}

func TestLocaleTags(t *testing.T) {
	for _, tag := range []string{"zh", "zh-CN", "zh_TW.UTF-8", "ZH-Hans", "zh_HK", "zh-Hant-MO", "zh.UTF-8", " zh @variant", "zh, en", "zh-like"} {
		if core.LanguageFromLocaleTag(tag) != core.Chinese {
			t.Errorf("Chinese: %q", tag)
		}
	}
	for _, tag := range []string{"en", "en-US", "en_GB.UTF-8", "ja-JP", "de_DE", "ru", "", "zhosomething", "zhz-CN", "-zh", "zℍ", "zh/EN"} {
		if core.LanguageFromLocaleTag(tag) != core.English {
			t.Errorf("English: %q", tag)
		}
	}
}

func TestLanguageJSON(t *testing.T) {
	for _, c := range []struct {
		l core.Language
		s string
	}{{core.Chinese, `"Zh"`}, {core.English, `"En"`}} {
		b, err := json.Marshal(c.l)
		if err != nil || string(b) != c.s {
			t.Fatalf("marshal: %s %v", b, err)
		}
		got := c.l.Toggle()
		if err := json.Unmarshal(b, &got); err != nil || got != c.l {
			t.Fatalf("unmarshal: %v %v", got, err)
		}
	}
	for _, s := range []string{`null`, `0`, `1`, `true`, `{}`, `[]`, `"zh"`, `"en"`, `"Chinese"`, `""`, `"Zh "`, `"Fr"`, `"Zh" "En"`} {
		got := core.English
		if err := json.Unmarshal([]byte(s), &got); err == nil {
			t.Errorf("accepted %s", s)
		}
		if got != core.English {
			t.Errorf("failed decode mutated receiver: %s", s)
		}
	}
	if _, err := json.Marshal(core.Language(2)); err == nil {
		t.Fatal("invalid enum marshalled")
	}
}

func TestText(t *testing.T) {
	for _, s := range []string{"%TEMP%", "Chrome", ""} {
		text := core.SameText(s)
		for _, l := range core.AllLanguages() {
			if text.Get(l) != s {
				t.Fatal("same text")
			}
		}
	}
	text := core.NewText("npm 缓存", "npm cache")
	if text.Get(core.Chinese) != "npm 缓存" || text.Get(core.English) != "npm cache" {
		t.Fatal("pair text")
	}
	text = core.Bilingual(func(l core.Language) string { return l.DisplayName() + "!" })
	if text.Get(core.Chinese) != "中文!" || text.Get(core.English) != "English!" {
		t.Fatal("bilingual text")
	}
	if (core.Text{}).Get(core.Chinese) != "" || (core.Text{}).Get(core.English) != "" {
		t.Fatal("zero text")
	}
}
