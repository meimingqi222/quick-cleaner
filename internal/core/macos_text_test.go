package core

import (
	"slices"
	"testing"
)

func TestMacResidualCommandEvidence(t *testing.T) {
	groups := parseApplicationGroups(`<plist><dict><key>com.apple.security.application-groups</key><array><string>group.com.example.app</string><string>group.com.example.app</string><string>../outside</string></array><key>other</key><array><string>group.com.other.app</string></array></dict></plist>`)
	if !slices.Equal(groups, []string{"group.com.example.app"}) {
		t.Fatalf("signed group evidence=%q", groups)
	}
	if groups := parseApplicationGroups(`<key>com.apple.security.application-groups</key><array><string>group.com.example.app</string>`); len(groups) != 0 {
		t.Fatalf("truncated entitlements=%q", groups)
	}
	extensions := parseSystemExtensions("*\t*\tTEAM\tcom.example.driver (1/1)\tname\t[activated enabled]\n*\t*\tTEAM\tcom.example.gone (1/1)\tname\t[terminated waiting to uninstall on reboot]\n[activated malformed]\n")
	if len(extensions) != 1 || extensions[0].TeamID != "TEAM" || extensions[0].BundleID != "com.example.driver" {
		t.Fatalf("active extensions=%+v", extensions)
	}
	labels := parseLaunchdRegistered(`
services = {
  0 0 com.example.app.idle
  - 1 COM.EXAMPLE.APP.started
  header com.example.app.fake
}
other = {
  12 0 com.example.app.foreign
}
disabled services = {
  "com.example.app.disabled" => disabled
  "com.example.app.updater" => enabled
}
`, "com.example.app")
	if !slices.Equal(labels, []string{"com.example.app.idle", "COM.EXAMPLE.APP.started", "com.example.app.updater"}) {
		t.Fatalf("registered occupancy=%q", labels)
	}
	for _, test := range []struct {
		line, name string
		match      bool
	}{
		{"12 /Applications/Mail.app/Contents/MacOS/Mail", "Mail", true},
		{"12 /Applications/MailMate.app/Contents/MacOS/MailMate", "Mail", false},
		{"12 Encoded", "Code", false},
		{"12 SENSEI", "Sensei", false},
		{"12 /apps/Sensei --agent", "Sensei", true},
		{"12 foreign --id COM.EXAMPLE.APP", "Other", true},
	} {
		if got := macResidualProcessMatches(test.line, "com.example.app", test.name); got != test.match {
			t.Fatalf("process %q name %q: %v want %v", test.line, test.name, got, test.match)
		}
	}
}
