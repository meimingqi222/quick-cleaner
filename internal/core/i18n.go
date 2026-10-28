package core

import (
	"encoding/json"
	"fmt"
	"strings"
)

type Language uint8

const (
	Chinese Language = iota
	English
)

// AllLanguages returns the supported languages in Rust's declaration order.
func AllLanguages() [2]Language { return [2]Language{Chinese, English} }
func (l Language) Code() string {
	if l == Chinese {
		return "zh"
	}
	return "en"
}
func (l Language) DisplayName() string {
	if l == Chinese {
		return "中文"
	}
	return "English"
}
func (l Language) ShortName() string {
	if l == Chinese {
		return "中"
	}
	return "EN"
}
func (l Language) Toggle() Language {
	if l == Chinese {
		return English
	}
	return Chinese
}
func LanguageFromLocaleTag(tag string) Language {
	if i := strings.IndexAny(tag, "-_.@,"); i >= 0 {
		tag = tag[:i]
	}
	tag = strings.TrimSpace(tag)
	if len(tag) == 2 && (tag[0] == 'z' || tag[0] == 'Z') && (tag[1] == 'h' || tag[1] == 'H') {
		return Chinese
	}
	return English
}
func (l Language) MarshalJSON() ([]byte, error) {
	switch l {
	case Chinese:
		return []byte(`"Zh"`), nil
	case English:
		return []byte(`"En"`), nil
	default:
		return nil, fmt.Errorf("unsupported language: %d", l)
	}
}
func (l *Language) UnmarshalJSON(data []byte) error {
	var value any
	if err := json.Unmarshal(data, &value); err != nil {
		return err
	}
	switch value {
	case "Zh":
		*l = Chinese
	case "En":
		*l = English
	default:
		return fmt.Errorf("unsupported language JSON: %s", data)
	}
	return nil
}

// Text freezes both translations so changing language never requires a rescan.
type Text struct{ zh, en string }

func NewText(zh, en string) Text { return Text{zh: zh, en: en} }
func SameText(s string) Text     { return Text{zh: s, en: s} }
func (t Text) Get(lang Language) string {
	if lang == Chinese {
		return t.zh
	}
	return t.en
}
func Bilingual(f func(Language) string) Text { return NewText(f(Chinese), f(English)) }
