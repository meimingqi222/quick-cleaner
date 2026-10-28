package platform

import (
	"path/filepath"
	"strings"
)

type VolumeID struct{ Mount, Label string }

func VolumeFromMountPoint(mount string) VolumeID { return VolumeID{filepath.Clean(mount), mount} }
func VolumeFromMountPointWithLabel(mount, label string) VolumeID {
	return VolumeID{filepath.Clean(mount), label}
}
func VolumeFromDriveLetter(letter byte) VolumeID {
	if letter >= 'a' && letter <= 'z' {
		letter -= 'a' - 'A'
	}
	return VolumeID{string(letter) + ":\\", string(letter) + ":"}
}
func (v VolumeID) Display() string    { return v.Label }
func (v VolumeID) String() string     { return v.Label }
func (v VolumeID) MountPoint() string { return v.Mount }
func (v VolumeID) DriveLetter() (byte, bool) {
	if len(v.Mount) >= 2 && v.Mount[1] == ':' {
		return strings.ToUpper(v.Mount[:1])[0], true
	}
	return 0, false
}
