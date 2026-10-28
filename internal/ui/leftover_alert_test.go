package ui

import (
	"testing"
)

func TestLeftoverAlertStrings(t *testing.T) {
	zh := leftoverAlertStrings(Chinese)
	if zh.title != "卸载残留检测" {
		t.Fatalf("unexpected zh title: %q", zh.title)
	}
	if zh.selectAll != "全选" || zh.selectSafe != "仅选常规" {
		t.Fatalf("unexpected zh selection labels: %q, %q", zh.selectAll, zh.selectSafe)
	}
	if zh.certain == "清除" {
		t.Fatalf("certain category should not be named with bare action verb '清除'")
	}
	if zh.confirm == "需要确认" {
		t.Fatalf("confirm category should have explanatory label")
	}

	en := leftoverAlertStrings(English)
	if en.title != "Leftover Detection" {
		t.Fatalf("unexpected en title: %q", en.title)
	}
	if en.selectAll != "Select all" || en.selectSafe != "Safe only" {
		t.Fatalf("unexpected en selection labels: %q, %q", en.selectAll, en.selectSafe)
	}
}

func TestLeftoverAlertSVGPreparsed(t *testing.T) {
	if alertTrashSVG == nil {
		t.Fatal("alertTrashSVG must be non-nil")
	}
	if alertCloseSVG == nil {
		t.Fatal("alertCloseSVG must be non-nil")
	}
	if alertBroomSVG == nil {
		t.Fatal("alertBroomSVG must be non-nil")
	}
	if alertLockSVG == nil {
		t.Fatal("alertLockSVG must be non-nil")
	}
	if alertInfoSVG == nil {
		t.Fatal("alertInfoSVG must be non-nil")
	}
	if alertRightSVG == nil {
		t.Fatal("alertRightSVG must be non-nil")
	}
}
