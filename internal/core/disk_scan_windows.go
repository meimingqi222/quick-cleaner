package core

import (
	"context"
	"log"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// ScanVolume never requests elevation or substitutes a partial directory walk
// for an unreadable raw NTFS volume.
func ScanVolume(ctx context.Context, volume VolumeID, topN int) (ScanResult, error) {
	started := time.Now()
	label := volume.Display()
	if letter, ok := volume.DriveLetter(); ok {
		label = string(letter)
	}
	raw, err := platform.ScanMFT(ctx, volume)
	if err != nil {
		log.Printf("MFT 解析 %s: 失败 %v，已耗时 %s", label, err, time.Since(started))
		return ScanResult{}, err
	}
	var built mftBuildTimes
	result, err := buildMFTScanResult(ctx, raw, topN, &built)
	if err != nil {
		log.Printf("MFT 解析 %s: 失败 %v，已耗时 %s", label, err, time.Since(started))
		return ScanResult{}, err
	}
	result.ElapsedMS = uint64(time.Since(started).Milliseconds())
	log.Printf(
		"MFT 解析 %s: 完成 %s（开卷 %s / 卷信息 %s / 首记录 %s / run list %s / 读盘 %s / 解析 %s / 扩展并回 %s / 聚合 %s / 建树 %s），扩展记录 %d，读入 %s，记录 %d/%d，%d 文件 / %d 目录，占用 %s",
		label, time.Since(started),
		raw.Timings.Open, raw.Timings.Volume, raw.Timings.First, raw.Timings.Runs, raw.Timings.Read, raw.Timings.Parse,
		built.Merge, built.Aggregate, built.Tree,
		result.ExtRecords, FmtSize(raw.BytesRead), result.RecordsRead, result.RecordsExpected,
		result.FileCount, result.DirCount, FmtSize(result.TotalSize),
	)
	return result, nil
}
