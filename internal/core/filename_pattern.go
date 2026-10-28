package core

import "strings"

// NamePattern uses ASCII case folding; ? consumes one Unicode rune, not one byte.
// Its zero value is the empty search pattern (which never matches a name).
type NamePattern struct {
	query    string
	wildcard bool
	literals []string
	chars    []rune
}

func ParseNamePattern(query string) NamePattern {
	p := NamePattern{query: asciiLower(query), wildcard: strings.ContainsAny(query, "*?")}
	if p.wildcard {
		p.chars = []rune(p.query)
		p.literals = strings.FieldsFunc(p.query, func(r rune) bool { return r == '*' || r == '?' })
	}
	return p
}

func (p NamePattern) Empty() bool { return p.query == "" }
func containsASCIIInsensitive(name, lower string) bool {
	for start := 0; start+len(lower) <= len(name); start++ {
		match := true
		for j := range len(lower) {
			b := name[start+j]
			if b >= 'A' && b <= 'Z' {
				b += 'a' - 'A'
			}
			if b != lower[j] {
				match = false
				break
			}
		}
		if match {
			return true
		}
	}
	return false
}
func (p NamePattern) MatchesRaw(name string) bool {
	if p.Empty() {
		return false
	}
	if !p.wildcard {
		return containsASCIIInsensitive(name, p.query)
	}
	for _, literal := range p.literals {
		if !containsASCIIInsensitive(name, literal) {
			return false
		}
	}
	return p.Matches(asciiLower(name))
}

// Matches expects an already ASCII-lowercased candidate.
func (p NamePattern) Matches(nameLower string) bool {
	if p.Empty() {
		return false
	}
	if !p.wildcard {
		return strings.Contains(nameLower, p.query)
	}
	for _, literal := range p.literals {
		if !strings.Contains(nameLower, literal) {
			return false
		}
	}
	pattern := p.chars
	prev := make([]bool, len(pattern)+1)
	prev[0] = true
	for j, r := range pattern {
		if r != '*' {
			break
		}
		prev[j+1] = true
	}
	curr := make([]bool, len(prev))
	for _, r := range nameLower {
		clear(curr)
		for j, pat := range pattern {
			if pat == '*' {
				curr[j+1] = prev[j+1] || curr[j]
			} else if pat == '?' || pat == r {
				curr[j+1] = prev[j]
			}
		}
		prev, curr = curr, prev
	}
	return prev[len(pattern)]
}
