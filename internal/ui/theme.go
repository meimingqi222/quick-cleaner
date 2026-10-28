package ui

import "github.com/egoist/mygo/ui"

// ShellBackground is the shell fill from src/ui/theme.rs. The native window
// uses it for any gap before the first frame; the frame itself is the same color.
const ShellBackground = "#f9f9f9"

// Values come from src/ui/theme.rs; do not substitute MyGo's default palette.
var (
	background       = ui.Hex(ShellBackground)
	card             = ui.Hex("#ffffff")
	surfaceLow       = ui.Hex("#f3f3f3")
	surfaceHigh      = ui.Hex("#e8e8e8")
	surfaceHighest   = ui.Hex("#e2e2e2")
	text             = ui.Hex("#1a1c1c")
	muted            = ui.Hex("#404752")
	outline          = ui.Hex("#717783")
	outlineVariant   = ui.Hex("#c0c7d4")
	primary          = ui.Hex("#005faa")
	primaryBright    = ui.Hex("#0078d4")
	primaryFixed     = ui.Hex("#d3e3ff")
	caution          = ui.Hex("#974700")
	cautionContainer = ui.Hex("#ffdbc8")
	success          = ui.Hex("#059669")
	successContainer = ui.Hex("#d1fae5")
)

func theme() *ui.Theme {
	t := ui.LightTheme()
	t.Background, t.Text, t.TextMuted = background, text, muted
	t.Accent, t.AccentText, t.Focus = primary, card, primary
	t.FontSize = 16
	return t
}
