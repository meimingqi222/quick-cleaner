package platform

import (
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"unicode/utf8"
)

func TestVolumeIDCompatibility(t *testing.T) {
	for _, mount := range []string{"/", "/Volumes/盘/../数据/", "", "relative/./path"} {
		v := VolumeFromMountPoint(mount)
		if v.MountPoint() != filepath.Clean(mount) || v.Display() != mount || v.String() != mount {
			t.Fatalf("mount constructor changed: %#v", v)
		}
		v = VolumeFromMountPointWithLabel(mount, "标签")
		if v.Mount != filepath.Clean(mount) || v.Label != "标签" || v.Display() != "标签" || v.String() != "标签" {
			t.Fatalf("label constructor changed: %#v", v)
		}
	}
	for _, letter := range []byte{'a', 'C', 'z'} {
		v := VolumeFromDriveLetter(letter)
		want := strings.ToUpper(string(letter))
		got, ok := v.DriveLetter()
		if !ok || got != want[0] || v.Mount != want+":\\" || v.Label != want+":" {
			t.Fatalf("drive constructor changed: %#v (%q, %v)", v, got, ok)
		}
	}
	if got, ok := (VolumeID{Mount: "c:/nested"}).DriveLetter(); !ok || got != 'C' {
		t.Fatalf("drive extraction = %q, %v", got, ok)
	}
	for _, mount := range []string{"/", "", "C", `\\server\share`} {
		if _, ok := (VolumeID{Mount: mount}).DriveLetter(); ok {
			t.Fatalf("unexpected drive letter for %q", mount)
		}
	}
}

func TestVolumeContainerDeviceKey(t *testing.T) {
	for device, want := range map[string]string{
		"/dev/disk3s3s1": "/dev/disk3", "/dev/disk3s5": "/dev/disk3",
		"/dev/disk3s3s1s2": "/dev/disk3", "/dev/disk4s1": "/dev/disk4",
		"server/share": "server/share", "/dev/disks": "/dev/disks",
		"/dev/disk3snap": "/dev/disk3snap", "/dev/disk3s１": "/dev/disk3s１",
	} {
		if got := containerDeviceKey(device); got != want {
			t.Errorf("containerDeviceKey(%q) = %q, want %q", device, got, want)
		}
	}
}

func TestVolumeSnapshotRecoveryGroupingAndDistinctDevices(t *testing.T) {
	mounts := []volumeMount{
		{mount: "/", device: "/dev/disk3s3s1", total: 100},
		{mount: "/Volumes/Macintosh HD", device: "/dev/disk3s1", label: "Macintosh HD", filesystem: "apfs", total: 100},
		{mount: "/Volumes/Recovery", device: "/dev/disk3s5", label: "Recovery", filesystem: "apfs", total: 100},
		{mount: "/Volumes/Other", device: "/dev/disk4s1", label: "Other", filesystem: "apfs", total: 100},
		{mount: "/Volumes/Large", device: "/dev/disk5s2", label: "Large", filesystem: "hfs", total: 200},
	}
	want := []VolumeID{
		VolumeFromMountPointWithLabel("/Volumes/Large", "Large"),
		VolumeFromMountPointWithLabel("/", "Macintosh HD"),
		VolumeFromMountPointWithLabel("/Volumes/Other", "Other"),
	}
	if got := groupMacVolumes(mounts); !reflect.DeepEqual(got, want) {
		t.Fatalf("grouped volumes = %#v, want %#v", got, want)
	}
	// Root preference is a property of identity, not the input ordering.
	mounts[0], mounts[1] = mounts[1], mounts[0]
	if got := groupMacVolumes(mounts); !reflect.DeepEqual(got, want) {
		t.Fatalf("late root = %#v, want %#v", got, want)
	}
}

func TestVolumeMacFilteringAndLabelSelection(t *testing.T) {
	mounts := []volumeMount{{mount: "/", device: "/dev/disk1s1", total: 100}}
	for _, label := range []string{"Recovery", "Preboot", "VM", "Update"} {
		mounts = append(mounts, volumeMount{mount: "/Volumes/" + label, device: "/dev/disk1s5", label: label, filesystem: "apfs", total: 100})
	}
	if got := groupMacVolumes(mounts); len(got) != 1 || got[0].Label != "/" {
		t.Fatalf("system labels replaced root: %#v", got)
	}
	for _, label := range []string{".secret", "backup.hidden-data", "NTFS"} {
		fs := "apfs"
		if label == "NTFS" {
			fs = "ntfs"
		}
		mounts = append(mounts, volumeMount{mount: "/Volumes/" + label, device: label, label: label, filesystem: fs, total: 300})
	}
	// Preserve Rust's byte-length selection: two Chinese runes beat five ASCII bytes.
	for _, label := range []string{"abcde", "中文", "other"} {
		mounts = append(mounts, volumeMount{mount: "/Volumes/" + label, device: "/dev/disk1s2", label: label, filesystem: "apfs", total: 100})
	}
	if got := groupMacVolumes(mounts); len(got) != 1 || got[0].Label != "中文" {
		t.Fatalf("filtered/selected labels = %#v", got)
	}
}

func TestVolumeMacUnicodeTruncation(t *testing.T) {
	for _, n := range []int{8, 22, 23, 30} {
		label := strings.Repeat("盘", n)
		got := groupMacVolumes([]volumeMount{{mount: "/Volumes/" + label, device: "/dev/disk7s1", label: label, filesystem: "apfs"}})
		want := label
		if n > 22 {
			want = strings.Repeat("盘", 22) + "…"
		}
		if len(got) != 1 || got[0].Label != want || !utf8.ValidString(got[0].Label) {
			t.Fatalf("Unicode truncation (%d) = %#v, want %q", n, got, want)
		}
	}
}
