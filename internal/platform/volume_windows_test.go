package platform

import (
	"strings"
	"testing"

	"golang.org/x/sys/windows"
)

func TestVolumeNativeWindowsFixedNTFS(t *testing.T) {
	volumes := ListVolumes()
	mask, err := windows.GetLogicalDrives()
	if err != nil {
		t.Fatal(err)
	}
	listed := make(map[byte]bool)
	var previous byte
	for _, vol := range volumes {
		letter, ok := vol.DriveLetter()
		if !ok || letter <= previous || vol != VolumeFromDriveLetter(letter) {
			t.Fatalf("unexpected volume/order: %#v", volumes)
		}
		listed[letter] = true
		previous = letter
		root, err := windows.UTF16PtrFromString(vol.MountPoint())
		if err != nil {
			t.Fatal(err)
		}
		var fs [32]uint16
		if windows.GetDriveType(root) != windows.DRIVE_FIXED {
			t.Fatalf("non-fixed drive: %#v", vol)
		}
		if err := windows.GetVolumeInformation(root, nil, 0, nil, nil, nil, &fs[0], uint32(len(fs))); err != nil {
			t.Fatal(err)
		}
		if !strings.EqualFold(windows.UTF16ToString(fs[:]), "NTFS") {
			t.Fatalf("non-NTFS drive: %#v", vol)
		}
		total, free, ok := GetVolumeSpace(vol)
		if !ok || total == 0 || free > total {
			t.Fatalf("space for %#v = %d, %d, %v", vol, total, free, ok)
		}
		// Queries normalize to the drive root, as the source does.
		nestedTotal, _, nestedOK := GetVolumeSpace(VolumeID{Mount: vol.Mount + "nonexistent/child"})
		if !nestedOK || nestedTotal != total {
			t.Fatalf("drive-root normalization failed: %d, %v", nestedTotal, nestedOK)
		}
	}
	for i := byte(0); i < 26; i++ {
		if mask&(1<<i) == 0 {
			continue
		}
		root, _ := windows.UTF16PtrFromString(VolumeFromDriveLetter('A' + i).MountPoint())
		var fs [32]uint16
		eligible := windows.GetDriveType(root) == windows.DRIVE_FIXED &&
			windows.GetVolumeInformation(root, nil, 0, nil, nil, nil, &fs[0], uint32(len(fs))) == nil &&
			strings.EqualFold(windows.UTF16ToString(fs[:]), "NTFS")
		if listed['A'+i] != eligible {
			t.Errorf("drive %c: listed=%v eligible=%v", 'A'+i, listed['A'+i], eligible)
		}
	}
}

func TestVolumeNativeWindowsRejectsNonDrive(t *testing.T) {
	for _, mount := range []string{"/", "", `\\server\share`, "\x00:"} {
		if total, free, ok := GetVolumeSpace(VolumeID{Mount: mount}); ok || total != 0 || free != 0 {
			t.Errorf("invalid drive %q: %d, %d, %v", mount, total, free, ok)
		}
	}
}
