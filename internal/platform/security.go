package platform

import "strings"

// QuoteWindowsArgument preserves the reference's CommandLineToArgvW quoting,
// including trailing backslashes and newline/vertical-tab arguments.
func QuoteWindowsArgument(arg string) string {
	if arg != "" && !strings.ContainsAny(arg, " \t\n\v\"") {
		return arg
	}
	var out strings.Builder
	out.WriteByte('"')
	slashes := 0
	for _, c := range arg {
		if c == '\\' {
			slashes++
			continue
		}
		if c == '"' {
			out.WriteString(strings.Repeat(`\`, slashes*2+1))
		} else {
			out.WriteString(strings.Repeat(`\`, slashes))
		}
		out.WriteRune(c)
		slashes = 0
	}
	out.WriteString(strings.Repeat(`\`, slashes*2))
	out.WriteByte('"')
	return out.String()
}

func elevationArgs(args []string, captured originalUser) []string {
	out := append([]string(nil), args...)
	has := func(flag string) bool {
		for _, arg := range args {
			if arg == flag {
				return true
			}
		}
		return false
	}
	if !has("--orig-user-home") && captured.hasHome {
		out = append(out, "--orig-user-home", captured.home)
	}
	if !has("--orig-user-sid") && captured.hasSID {
		out = append(out, "--orig-user-sid", captured.sid)
	}
	return out
}
