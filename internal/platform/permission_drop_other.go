//go:build !darwin

package platform

// FDA drop panel is an AppKit NSPanel. Other platforms have no equivalent.

type FDADropText struct{ AppName, Line1, Line2 string }

func ShowFDADrop(string, FDADropText, bool) bool { return false }
func HideFDADrop()                               {}
func FDADropVisible() bool                       { return false }
