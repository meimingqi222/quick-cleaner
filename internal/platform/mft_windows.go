//go:build windows

package platform

import (
	"context"
	"time"

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
	opened := time.Now()
	h, err := windows.CreateFile(path, windows.GENERIC_READ, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE, nil, windows.OPEN_EXISTING, 0, 0)
	open := time.Since(opened)
	if err != nil {
		return MFTScanResult{}, err
	}
	defer windows.CloseHandle(h)
	// os.File.ReadAt puts the handle on the Windows poller and issues overlapped
	// I/O with a seek dance. Raw ReadFile matches the Rust volume read.
	probed := time.Now()
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
	vol := rawVolume{h}
	if err = mftReadAt(ctx, vol, boot, 0); err != nil {
		return MFTScanResult{}, err
	}
	// Compare independent boot-sector geometry to the FSCTL response before
	// trusting either for physical offsets or allocations.
	if err = mftValidateBoot(boot, g); err != nil {
		return MFTScanResult{}, err
	}
	volumeTime := time.Since(probed)
	scanned, err := mftScanReader(ctx, vol, volume, g)
	scanned.Timings.Open = open
	scanned.Timings.Volume = volumeTime
	return scanned, err
}

// rawVolume reads with the file pointer. One scanner owns the handle; the
// pointer is not safe for concurrent ReadAt calls.
type rawVolume struct{ h windows.Handle }

func (v rawVolume) ReadAt(p []byte, off int64) (int, error) {
	if len(p) == 0 {
		return 0, nil
	}
	if off < 0 {
		return 0, ErrMFTCorrupt
	}
	if _, err := windows.Seek(v.h, off, 0); err != nil {
		return 0, err
	}
	var n uint32
	err := windows.ReadFile(v.h, p, &n, nil)
	return int(n), err
}
