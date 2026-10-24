//go:build windows

package platform

import (
	"context"
	"os"

	"golang.org/x/sys/windows"
)

// ScanMFT opens a raw volume read-only. It never elevates, walks directories,
// or mutates the filesystem. Cancellation is checked between synchronous reads.
func ScanMFT(ctx context.Context, volume VolumeID) (MFTScanResult, error) {
	if err := ctx.Err(); err != nil {
		return MFTScanResult{}, err
	}
	letter, ok := volume.DriveLetter()
	if !ok || letter < 'A' || letter > 'Z' {
		return MFTScanResult{}, ErrMFTUnsupported
	}
	path, _ := windows.UTF16PtrFromString(`\\.\` + string(letter) + ":")
	h, err := windows.CreateFile(path, windows.GENERIC_READ, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE, nil, windows.OPEN_EXISTING, 0, 0)
	if err != nil {
		return MFTScanResult{}, err
	}
	f := os.NewFile(uintptr(h), "NTFS volume")
	defer f.Close()
	var data [96]byte
	var returned uint32
	if err = windows.DeviceIoControl(h, 0x00090064, nil, 0, &data[0], uint32(len(data)), &returned, nil); err != nil {
		return MFTScanResult{}, err
	}
	if returned < 96 {
		return MFTScanResult{}, ErrMFTCorrupt
	}
	g := mftGeometry{Sector: uint64(mftLE.Uint32(data[40:])), Cluster: uint64(mftLE.Uint32(data[44:])), Record: uint64(mftLE.Uint32(data[48:])), Valid: mftLE.Uint64(data[56:]), Start: mftLE.Uint64(data[64:])}
	if err = g.validate(); err != nil {
		return MFTScanResult{}, err
	}
	boot := make([]byte, int(g.Sector))
	if err = mftReadAt(ctx, f, boot, 0); err != nil {
		return MFTScanResult{}, err
	}
	// Compare independent boot-sector geometry to the FSCTL response before
	// trusting either for physical offsets or allocations.
	if err = mftValidateBoot(boot, g); err != nil {
		return MFTScanResult{}, err
	}
	return mftScanReader(ctx, f, volume, g)
}
