package platform

import (
	"strings"

	"golang.org/x/sys/windows"
)

func ListVolumes() []VolumeID {
	mask, err := windows.GetLogicalDrives()
	if err != nil {
		return nil
	}
	var out []VolumeID
	for i := byte(0); i < 26; i++ {
		if mask&(1<<i) == 0 {
			continue
		}
		vol := VolumeFromDriveLetter('A' + i)
		root, _ := windows.UTF16PtrFromString(vol.MountPoint())
		if windows.GetDriveType(root) != windows.DRIVE_FIXED {
			continue
		}
		var fs [32]uint16
		if windows.GetVolumeInformation(root, nil, 0, nil, nil, nil, &fs[0], uint32(len(fs))) != nil {
			continue
		}
		if strings.EqualFold(windows.UTF16ToString(fs[:]), "NTFS") {
			out = append(out, vol)
		}
	}
	return out
}

func GetVolumeSpace(vol VolumeID) (total, free uint64, ok bool) {
	letter, valid := vol.DriveLetter()
	if !valid {
		return 0, 0, false
	}
	root, err := windows.UTF16PtrFromString(VolumeFromDriveLetter(letter).MountPoint())
	if err != nil {
		return 0, 0, false
	}
	var available uint64
	// Preserve the source's total-free result, not caller/quota-available space.
	if windows.GetDiskFreeSpaceEx(root, &available, &total, &free) != nil {
		return 0, 0, false
	}
	return total, free, true
}
