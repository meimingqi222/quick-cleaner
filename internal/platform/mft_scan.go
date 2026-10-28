package platform

import (
	"context"
	"io"
	"math"
	"runtime"
	"sync"
	"time"
)

type mftGeometry struct{ Sector, Cluster, Record, Start, Valid uint64 }

func (g mftGeometry) validate() error {
	if g.Sector < 512 || g.Sector > 4096 || g.Sector&(g.Sector-1) != 0 || g.Cluster < g.Sector || g.Cluster > 2*1024*1024 || g.Cluster&(g.Cluster-1) != 0 || g.Record < g.Sector || g.Record > 64*1024 || g.Record&(g.Record-1) != 0 || g.Valid/g.Record <= MFTRootRecord || g.Valid > math.MaxInt64 || g.Start > math.MaxInt64/g.Cluster {
		return ErrMFTCorrupt
	}
	return nil
}

func mftValidateBoot(boot []byte, g mftGeometry) error {
	if len(boot) < 512 || string(boot[3:11]) != "NTFS    " {
		return ErrMFTUnsupported
	}
	encoded := int8(boot[64])
	var record uint64
	if encoded < 0 {
		if encoded < -16 {
			return ErrMFTCorrupt
		}
		record = uint64(1) << uint(-encoded)
	} else {
		record = uint64(encoded) * g.Cluster
	}
	if uint64(mftLE.Uint16(boot[11:])) != g.Sector || uint64(boot[13])*g.Sector != g.Cluster || record != g.Record || mftLE.Uint64(boot[48:]) != g.Start {
		return ErrMFTCorrupt
	}
	return nil
}

func mftReadAt(ctx context.Context, r io.ReaderAt, b []byte, offset uint64) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if offset > math.MaxInt64 || uint64(len(b)) > math.MaxInt64-offset {
		return ErrMFTCorrupt
	}
	n, err := r.ReadAt(b, int64(offset))
	if err != nil {
		return err
	}
	if n != len(b) {
		return io.ErrUnexpectedEOF
	}
	return ctx.Err()
}

// Read logical stream bytes, retaining sparse holes and splitting at physical
// run boundaries. Never concatenate physical runs and renumber the records.
func mftReadStream(ctx context.Context, r io.ReaderAt, runs []mftRun, cluster, offset uint64, b []byte) error {
	if cluster == 0 {
		return ErrMFTCorrupt
	}
	for len(b) > 0 {
		if err := ctx.Err(); err != nil {
			return err
		}
		vcn := offset / cluster
		found := false
		for _, run := range runs {
			if vcn < run.VCN || vcn-run.VCN >= run.Clusters {
				continue
			}
			found = true
			left := run.Clusters - (vcn - run.VCN)
			if left > math.MaxUint64/cluster {
				return ErrMFTCorrupt
			}
			n := min(uint64(len(b)), left*cluster-offset%cluster)
			if run.Sparse {
				clear(b[:int(n)])
			} else {
				if run.LCN > math.MaxUint64-(vcn-run.VCN) {
					return ErrMFTCorrupt
				}
				lcn := run.LCN + vcn - run.VCN
				if lcn > math.MaxInt64/cluster {
					return ErrMFTCorrupt
				}
				if err := mftReadAt(ctx, r, b[:int(n)], lcn*cluster+offset%cluster); err != nil {
					return err
				}
			}
			b = b[int(n):]
			offset += n
			break
		}
		if !found {
			return ErrMFTCorrupt
		}
	}
	return nil
}
func mftReadList(ctx context.Context, r io.ReaderAt, rec []byte, g mftGeometry) ([]uint64, error) {
	attrs, err := mftAttributes(rec)
	if err != nil {
		return nil, err
	}
	for _, a := range attrs {
		if a.typ != 0x20 {
			continue
		}
		if !a.nonresident {
			return mftListRecords(a.value())
		}
		n := mftLE.Uint64(a.b[48:])
		if n > mftMaxAttrList {
			return nil, ErrMFTCorrupt
		}
		runs, err := a.runs()
		if err != nil {
			return nil, err
		}
		if g.Cluster == 0 || g.Cluster > 2*1024*1024 {
			return nil, ErrMFTCorrupt
		}
		// Raw volume reads must be sector-aligned even when the list's logical
		// value ends mid-sector. Its allocated runs cover whole clusters.
		padded := (n + g.Cluster - 1) / g.Cluster * g.Cluster
		b := make([]byte, int(padded))
		if err = mftReadStream(ctx, r, runs, g.Cluster, 0, b); err != nil {
			return nil, err
		}
		return mftListRecords(b[:int(n)])
	}
	return nil, nil
}

func mftScanReader(ctx context.Context, r io.ReaderAt, volume VolumeID, g mftGeometry) (MFTScanResult, error) {
	result := MFTScanResult{Volume: volume, RootRecord: MFTRootRecord}
	if err := g.validate(); err != nil {
		return result, err
	}
	started := time.Now()
	first := make([]byte, int(g.Record))
	if err := mftReadAt(ctx, r, first, g.Start*g.Cluster); err != nil {
		return result, err
	}
	result.Timings.First = time.Since(started)
	if err := mftFixup(first, g.Sector); err != nil {
		return result, err
	}
	started = time.Now()
	runs, err := mftDataRuns(first)
	if err != nil {
		return result, err
	}
	if err = mftOrderRuns(runs); err != nil {
		return result, err
	}
	pending, err := mftReadList(ctx, r, first, g)
	if err != nil {
		return result, err
	}
	// Retry extensions after adding each fragment: an earlier extension can make
	// another extension's logical record address readable.
	for len(pending) > 0 {
		progress := false
		next := make([]uint64, 0, len(pending))
		for _, no := range pending {
			if no >= g.Valid/g.Record {
				return result, ErrMFTCorrupt
			}
			ext := make([]byte, int(g.Record))
			if err = mftReadStream(ctx, r, runs, g.Cluster, no*g.Record, ext); err != nil {
				if ctx.Err() != nil {
					return result, ctx.Err()
				}
				next = append(next, no)
				continue
			}
			if err = mftFixup(ext, g.Sector); err != nil {
				return result, err
			}
			if mftLE.Uint64(ext[32:])&mftRefMask != 0 || mftLE.Uint16(ext[22:])&1 == 0 {
				return result, ErrMFTCorrupt
			}
			more, e := mftDataRuns(ext)
			if e != nil {
				return result, e
			}
			if len(more) > 0 {
				result.ExtRecords++
			}
			runs = append(runs, more...)
			if err = mftOrderRuns(runs); err != nil {
				return result, err
			}
			progress = true
		}
		if !progress {
			return result, ErrMFTCorrupt
		}
		pending = next
	}
	result.Timings.Runs = time.Since(started)
	for _, run := range runs {
		if run.Clusters > (math.MaxUint64-result.MFTRunBytes)/g.Cluster {
			return result, ErrMFTCorrupt
		}
		result.MFTRunBytes += run.Clusters * g.Cluster
	}
	// Like Rust, read up to the smaller of the valid length and the allocated
	// runs; large volumes legitimately exceed any fixed record count. Tree
	// indexes are uint32. Entries grow per chunk, so a corrupt run list fails
	// on its first unreadable chunk instead of allocating its claimed size.
	count := min(g.Valid, result.MFTRunBytes) / g.Record
	if count > math.MaxUint32 {
		return result, ErrMFTCorrupt
	}
	result.RecordsExpected = count
	result.Entries = make([]MFTEntry, 0, min(count, 1<<22))
	buf := make([]byte, 8*1024*1024)
	recSize := int(g.Record)
	for no := uint64(0); no < count; {
		if err = ctx.Err(); err != nil {
			return result, err
		}
		n := min(uint64(len(buf))/g.Record, count-no)
		result.Entries = append(result.Entries, make([]MFTEntry, n)...)
		chunk := buf[:int(n*g.Record)]
		read := time.Now()
		if err = mftReadStream(ctx, r, runs, g.Cluster, no*g.Record, chunk); err != nil {
			return result, err
		}
		result.Timings.Read += time.Since(read)
		parsed := time.Now()
		decodeMFTChunk(chunk, recSize, g.Sector, result.Entries[no:no+n])
		result.Timings.Parse += time.Since(parsed)
		result.BytesRead += uint64(len(chunk))
		no += n
	}
	return result, nil
}

// decodeMFTChunk parses independent records in order. Rust splits the same
// 8MiB chunk across cores; a single thread spends most of the scan here.
func decodeMFTChunk(chunk []byte, recSize int, sector uint64, out []MFTEntry) {
	n := len(out)
	workers := runtime.GOMAXPROCS(0)
	if workers > n {
		workers = n
	}
	if workers < 2 || n < 128 {
		decodeMFTRecords(chunk, recSize, sector, out, 0, n)
		return
	}
	var wg sync.WaitGroup
	span := (n + workers - 1) / workers
	for w := 0; w < workers; w++ {
		start := w * span
		if start >= n {
			break
		}
		end := min(start+span, n)
		wg.Add(1)
		go func(start, end int) {
			defer wg.Done()
			decodeMFTRecords(chunk, recSize, sector, out, start, end)
		}(start, end)
	}
	wg.Wait()
}

func decodeMFTRecords(chunk []byte, recSize int, sector uint64, out []MFTEntry, start, end int) {
	var scratch mftDecodeScratch
	scratch.parents = make([]uint64, 0, end-start)
	for i := start; i < end; i++ {
		rec := chunk[i*recSize : (i+1)*recSize]
		if mftFixup(rec, sector) == nil {
			out[i], _ = mftDecodeRecordScratch(rec, &scratch)
		}
	}
}
