package core

import (
	"slices"
	"strings"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func parseApplicationGroups(entitlements string) []string {
	_, after, ok := strings.Cut(entitlements, "<key>com.apple.security.application-groups</key>")
	if !ok {
		return nil
	}
	array, _, ok := strings.Cut(after, "</array>")
	if !ok {
		return nil
	}
	var groups []string
	for {
		_, value, ok := strings.Cut(array, "<string>")
		if !ok {
			break
		}
		group, rest, ok := strings.Cut(value, "</string>")
		if !ok {
			break
		}
		if platform.ValidBundleID(group) && !slices.Contains(groups, group) {
			groups = append(groups, group)
		}
		array = rest
	}
	return groups
}

func parseSystemExtensions(output string) []ResidualKind {
	var extensions []ResidualKind
	for line := range strings.SplitSeq(output, "\n") {
		if !strings.Contains(line, "[activated") {
			continue
		}
		fields := strings.Split(line, "\t")
		if len(fields) < 4 {
			continue
		}
		team := strings.TrimSpace(fields[2])
		bundle := strings.Fields(fields[3])
		if team != "" && len(bundle) > 0 && platform.ValidBundleID(bundle[0]) {
			extensions = append(extensions, ResidualKind{Type: ResidualSystemExtension, TeamID: team, BundleID: bundle[0]})
		}
	}
	return extensions
}

func parseLaunchdRegistered(output, needle string) []string {
	if needle == "" {
		return nil
	}
	var labels []string
	section := ""
	for line := range strings.SplitSeq(output, "\n") {
		trimmed := strings.TrimLeft(line, " \t\r")
		switch trimmed {
		case "services = {":
			section = "services"
			continue
		case "disabled services = {":
			section = "disabled"
			continue
		case "}":
			section = ""
			continue
		}
		label := ""
		switch section {
		case "services":
			fields := strings.Fields(trimmed)
			if len(fields) >= 3 && (fields[0] == "-" || macDecimal(fields[0])) {
				label = fields[len(fields)-1]
			}
		case "disabled":
			if strings.Contains(trimmed, "=> enabled") {
				if _, after, ok := strings.Cut(trimmed, `"`); ok {
					label, _, _ = strings.Cut(after, `"`)
				}
			}
		}
		if strings.Contains(asciiLower(label), needle) && !slices.Contains(labels, label) {
			labels = append(labels, label)
		}
	}
	return labels
}

func macDecimal(value string) bool {
	if value == "" {
		return false
	}
	for _, c := range []byte(value) {
		if c < '0' || c > '9' {
			return false
		}
	}
	return true
}

func macResidualProcessMatches(line, bundleLower, displayName string) bool {
	if bundleLower != "" && strings.Contains(asciiLower(line), bundleLower) {
		return true
	}
	name := strings.TrimSpace(displayName)
	if len(name) < 4 || !IsSafeAppToken(name) {
		return false
	}
	for offset := 0; offset <= len(line)-len(name); {
		index := strings.Index(line[offset:], name)
		if index < 0 {
			break
		}
		start := offset + index
		end := start + len(name)
		if (start == 0 || !macASCIIAlphanumeric(line[start-1])) && (end == len(line) || !macASCIIAlphanumeric(line[end])) {
			return true
		}
		offset = end
	}
	return false
}

func macASCIIAlphanumeric(c byte) bool {
	return c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9'
}
