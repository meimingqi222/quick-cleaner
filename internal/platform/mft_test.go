package platform

import (
	"bytes"
	"context"
	"errors"
	"io"
	"math/rand"
	"reflect"
	"testing"
	"unicode/utf16"
)

func mftTestResident(typ uint32, value []byte) []byte {
	a := make([]byte, (24+len(value)+7)&^7)
	mftLE.PutUint32(a, typ)
	mftLE.PutUint32(a[4:], uint32(len(a)))
	mftLE.PutUint32(a[16:], uint32(len(value)))
	mftLE.PutUint16(a[20:], 24)
	copy(a[24:], value)
	return a
}
func mftTestData(vcn, size uint64, runs []byte) []byte {
	a := make([]byte, (64+len(runs)+7)&^7)
	mftLE.PutUint32(a, 0x80)
	mftLE.PutUint32(a[4:], uint32(len(a)))
	a[8] = 1
	mftLE.PutUint64(a[16:], vcn)
	mftLE.PutUint16(a[32:], 64)
	mftLE.PutUint64(a[48:], size)
	copy(a[64:], runs)
	return a
}
func mftTestName(parent uint64, name string, ns byte) []byte {
	u := utf16.Encode([]rune(name))
	v := make([]byte, 66+len(u)*2)
	mftLE.PutUint64(v, parent)
	v[64] = byte(len(u))
	v[65] = ns
	for i, n := range u {
		mftLE.PutUint16(v[66+i*2:], n)
	}
	return mftTestResident(0x30, v)
}
func mftTestRecord(base uint64, dir bool, attrs ...[]byte) []byte {
	b := make([]byte, 1024)
	copy(b, "FILE")
	mftLE.PutUint16(b[4:], 48)
	mftLE.PutUint16(b[6:], 3)
	mftLE.PutUint16(b[20:], 56)
	mftLE.PutUint16(b[22:], 1)
	if dir {
		mftLE.PutUint16(b[22:], 3)
	}
	mftLE.PutUint64(b[32:], base)
	p := 56
	for _, a := range attrs {
		copy(b[p:], a)
		p += len(a)
	}
	mftLE.PutUint32(b[p:], 0xffffffff)
	return b
}
func mftTestProtect(b []byte) []byte {
	b = bytes.Clone(b)
	mftLE.PutUint16(b[48:], 0xbbaa)
	copy(b[50:52], b[510:512])
	copy(b[52:54], b[1022:1024])
	mftLE.PutUint16(b[510:], 0xbbaa)
	mftLE.PutUint16(b[1022:], 0xbbaa)
	return b
}
func mftTestList(typ uint32, no uint64, named bool) []byte {
	b := make([]byte, 32)
	mftLE.PutUint32(b, typ)
	mftLE.PutUint16(b[4:], 32)
	mftLE.PutUint64(b[16:], no)
	if named {
		b[6] = 1
	}
	return b
}

func TestMFTRunsSignedSparseAndMalformed(t *testing.T) {
	b := []byte{0x11, 16, 100, 0x01, 8, 0x81, 4}
	b = append(b, 0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0)
	got, err := mftParseRuns(b, 20)
	want := []mftRun{{20, 100, 16, false}, {36, 0, 8, true}, {44, 98, 4, false}}
	if err != nil || !reflect.DeepEqual(got, want) {
		t.Fatalf("runs=%+v err=%v", got, err)
	}
	for _, bad := range [][]byte{{0xf1, 1}, {0x1f, 1}, {0x10}, {0x11, 0, 3, 0}, {0x11, 1, 0xff, 0}, {0x11, 1, 2}} {
		if _, e := mftParseRuns(bad, 0); e == nil {
			t.Fatalf("accepted %x", bad)
		}
	}
	got, err = mftParseRuns([]byte{0x11, 1, 2, 0, 0xff}, 0)
	if err != nil || len(got) != 1 {
		t.Fatal(got, err)
	}
}
func TestMFTFixupRequiresEverySectorAndRestores(t *testing.T) {
	b := mftTestRecord(0, false)
	mftLE.PutUint16(b[510:], 0x1122)
	mftLE.PutUint16(b[1022:], 0x3344)
	protected := mftTestProtect(b)
	if err := mftFixup(protected, 512); err != nil || mftLE.Uint16(protected[510:]) != 0x1122 || mftLE.Uint16(protected[1022:]) != 0x3344 {
		t.Fatal("fixup", err)
	}
	for _, mutate := range []func([]byte){func(b []byte) { b[1022] ^= 1 }, func(b []byte) { mftLE.PutUint16(b[6:], 2) }, func(b []byte) { mftLE.PutUint16(b[4:], 1023) }} {
		bad := mftTestProtect(b)
		mutate(bad)
		if mftFixup(bad, 512) == nil {
			t.Fatal("accepted torn/incomplete USA")
		}
	}
}
func TestMFTRecordNamesStreamsExtensionsAndBounds(t *testing.T) {
	si := make([]byte, 32)
	mftLE.PutUint64(si[8:], 116444736000000000+123*10000000)
	named := mftTestResident(0x80, []byte("ignored"))
	named[9] = 1
	mftLE.PutUint16(named[10:], 24)
	rec := mftTestRecord(0, false, mftTestName(6, "SHORT~1", 2), mftTestName(7, "文件😀", 1), mftTestResident(0x10, si), mftTestData(0, 900, []byte{0}), mftTestData(8, 999999, []byte{0}), named)
	e, err := mftDecodeRecord(rec)
	if err != nil || !e.Used || e.Name != "文件😀" || e.Parent != 7 || e.Size != 900 || e.Mtime != 123 || !reflect.DeepEqual(e.LinkParents, []uint64{6, 7}) {
		t.Fatalf("entry=%+v err=%v", e, err)
	}
	ext := mftTestRecord(42, false, mftTestName(8, "link", 1), mftTestResident(0x80, []byte("abc")))
	e, err = mftDecodeRecord(ext)
	if err != nil || e.Used || e.BaseRecord != 42 || e.Size != 3 || !reflect.DeepEqual(e.LinkParents, []uint64{8}) {
		t.Fatal(e, err)
	}
	dir := mftTestRecord(0, true, mftTestName(5, "folder", 1), mftTestResident(0x80, []byte("abc")))
	e, err = mftDecodeRecord(dir)
	if err != nil || !e.IsDir || e.Size != 0 {
		t.Fatal(e, err)
	}
	for _, mutate := range []func([]byte){
		func(b []byte) { mftLE.PutUint16(b[20:], 65535) },
		func(b []byte) { mftLE.PutUint32(b[60:], 16) },
		func(b []byte) { mftLE.PutUint32(b[60:], 0) },
		func(b []byte) { mftLE.PutUint32(b[60:], 0xffffffff) },
		func(b []byte) { mftLE.PutUint16(b[76:], 900) },
		func(b []byte) { b[56+24+64] = 255 },
	} {
		b := bytes.Clone(rec)
		mutate(b)
		if _, err = mftDecodeRecord(b); err == nil {
			t.Fatal("accepted malformed attribute")
		}
	}
	bad := mftTestRecord(0, false, mftTestData(0, 1, []byte{0}))
	mftLE.PutUint32(bad[60:], 16)
	if _, err = mftDecodeRecord(bad); err == nil {
		t.Fatal("short nonresident header")
	}
}
func TestMFTPhysicalRunLocationsAndSparseHole(t *testing.T) {
	disk := make([]byte, 64)
	copy(disk[40:], "ABCD")
	copy(disk[8:], "WXYZ")
	runs := []mftRun{{2, 2, 1, false}, {0, 10, 1, false}, {1, 0, 1, true}}
	if err := mftOrderRuns(runs); err != nil {
		t.Fatal(err)
	}
	b := make([]byte, 12)
	if err := mftReadStream(context.Background(), bytes.NewReader(disk), runs, 4, 0, b); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(b, []byte{'A', 'B', 'C', 'D', 0, 0, 0, 0, 'W', 'X', 'Y', 'Z'}) {
		t.Fatalf("wrong VCN/LCN interpretation: %q", b)
	}
	if mftOrderRuns([]mftRun{{0, 10, 2, false}, {1, 20, 1, false}}) == nil {
		t.Fatal("overlap accepted")
	}
	if mftReadStream(context.Background(), bytes.NewReader(disk), []mftRun{{1, 2, 1, false}}, 4, 0, b) == nil {
		t.Fatal("gap concatenated away")
	}
}
func TestMFTAttributeListResidentAndNonresident(t *testing.T) {
	list := append(mftTestList(0x80, 42, false), mftTestList(0x80, 77, false)...)
	list = append(list, mftTestList(0x80, 42, false)...)
	list = append(list, mftTestList(0x80, 99, true)...)
	list = append(list, mftTestList(0x10, 88, false)...)
	for _, nonresident := range []bool{false, true} {
		a := mftTestResident(0x20, list)
		disk := make([]byte, 1024)
		copy(disk[512:], list)
		if nonresident {
			a = mftTestData(0, uint64(len(list)), []byte{0x11, 1, 1, 0})
			mftLE.PutUint32(a, 0x20)
		}
		got, err := mftReadList(context.Background(), mftTestAlignedReader{bytes.NewReader(disk)}, mftTestRecord(0, false, a), mftGeometry{Cluster: 512})
		if err != nil || !reflect.DeepEqual(got, []uint64{42, 77}) {
			t.Fatal(got, err)
		}
	}
	a := mftTestData(0, mftMaxAttrList+1, []byte{0})
	mftLE.PutUint32(a, 0x20)
	if _, err := mftReadList(context.Background(), bytes.NewReader(nil), mftTestRecord(0, false, a), mftGeometry{Cluster: 512}); err == nil {
		t.Fatal("unbounded list")
	}
}

type mftTestAlignedReader struct{ io.ReaderAt }

func (r mftTestAlignedReader) ReadAt(b []byte, offset int64) (int, error) {
	if offset%512 != 0 || len(b)%512 != 0 {
		return 0, errors.New("unaligned raw volume read")
	}
	return r.ReaderAt.ReadAt(b, offset)
}

func TestMFTBootAndGeometry(t *testing.T) {
	g := mftGeometry{Sector: 512, Cluster: 4096, Record: 1024, Start: 12, Valid: 8192}
	boot := make([]byte, 512)
	copy(boot[3:], "NTFS    ")
	mftLE.PutUint16(boot[11:], 512)
	boot[13] = 8
	boot[64] = 246 // -10: 2^10 bytes, not ten clusters.
	mftLE.PutUint64(boot[48:], 12)
	if err := g.validate(); err != nil {
		t.Fatal(err)
	}
	if err := mftValidateBoot(boot, g); err != nil {
		t.Fatal(err)
	}
	for _, mutate := range []func([]byte){
		func(b []byte) { b[3] = 'X' },
		func(b []byte) { b[13] = 4 },
		func(b []byte) { b[64] = 128 },
		func(b []byte) { b[48] = 13 },
	} {
		b := bytes.Clone(boot)
		mutate(b)
		if mftValidateBoot(b, g) == nil {
			t.Fatal("mismatched boot geometry accepted")
		}
	}
	boot[64] = 1
	g.Record = g.Cluster
	if err := mftValidateBoot(boot, g); err != nil {
		t.Fatal("positive cluster record size", err)
	}
	for _, bad := range []mftGeometry{{}, {512, 0, 1024, 0, 8192}, {512, 4096, 0, 0, 8192}, {512, 4096, 1024, ^uint64(0), 8192}, {512, 4096, 1024, 1, ^uint64(0)}} {
		if bad.validate() == nil {
			t.Fatal("unsafe geometry accepted", bad)
		}
	}
}

type mftTestCancelReader struct {
	io.ReaderAt
	cancel context.CancelFunc
}

func (r mftTestCancelReader) ReadAt(b []byte, offset int64) (int, error) {
	n, err := r.ReaderAt.ReadAt(b, offset)
	r.cancel()
	return n, err
}

func TestMFTCancelAfterRead(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	r := mftTestCancelReader{bytes.NewReader(make([]byte, 512)), cancel}
	if err := mftReadAt(ctx, r, make([]byte, 512), 0); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func TestMFTScanFixtureExtensionFragmentsAndRecordIndexes(t *testing.T) {
	g := mftGeometry{Sector: 512, Cluster: 512, Record: 1024, Start: 10, Valid: 8 * 1024}
	disk := make([]byte, 40*512)
	first := mftTestRecord(0, false, mftTestData(0, g.Valid, []byte{0x11, 8, 10, 0}), mftTestResident(0x20, mftTestList(0x80, 1, false)))
	ext := mftTestRecord(0x1000000000000, false, mftTestData(8, 0, []byte{0x11, 8, 30, 0}))
	copy(disk[10*512:], mftTestProtect(first))
	copy(disk[12*512:], mftTestProtect(ext))
	copy(disk[32*512:], mftTestProtect(mftTestRecord(0, true, mftTestName(5, ".", 1))))
	copy(disk[34*512:], mftTestProtect(mftTestRecord(0, false, mftTestName(5, "tail.bin", 1), mftTestResident(0x80, []byte("hello")))))
	res, err := mftScanReader(context.Background(), bytes.NewReader(disk), VolumeFromDriveLetter('D'), g)
	if err != nil || len(res.Entries) != 8 || res.RootRecord != 5 || !res.Entries[5].IsDir || res.Entries[6].Name != "tail.bin" || res.Entries[6].Size != 5 || res.Entries[4].Used || res.Volume != VolumeFromDriveLetter('D') {
		t.Fatalf("result=%+v err=%v", res, err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err = mftScanReader(ctx, bytes.NewReader(disk), VolumeID{}, g); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if _, err = mftScanReader(context.Background(), bytes.NewReader(disk[:6000]), VolumeID{}, g); err == nil {
		t.Fatal("short physical read accepted")
	}
}

// The volume's valid length can exceed both any fixed record count and the
// allocated runs. Rust reads what the runs hold; a large NTFS volume must not
// fail as corrupt.
func TestMFTRecordCountFollowsRunsNotAFixedCap(t *testing.T) {
	g := mftGeometry{Sector: 512, Cluster: 512, Record: 1024, Start: 10, Valid: 8 * 1024}
	disk := make([]byte, 40*512)
	first := mftTestRecord(0, false, mftTestData(0, g.Valid, []byte{0x11, 16, 10, 0}))
	copy(disk[10*512:], mftTestProtect(first))
	copy(disk[20*512:], mftTestProtect(mftTestRecord(0, true, mftTestName(5, ".", 1))))
	g.Valid = 32 * 1024 * 1024 * 1024
	res, err := mftScanReader(context.Background(), bytes.NewReader(disk), VolumeFromDriveLetter('D'), g)
	if err != nil {
		t.Fatalf("valid length beyond a fixed cap rejected: %v", err)
	}
	if len(res.Entries) != 8 || res.RecordsExpected != 8 || !res.Entries[5].IsDir {
		t.Fatalf("records must follow the allocated runs: %d expected=%d", len(res.Entries), res.RecordsExpected)
	}
}

func TestMFTArbitraryBytesNeverPanic(t *testing.T) {
	r := rand.New(rand.NewSource(123))
	for i := 0; i < 2000; i++ {
		b := make([]byte, r.Intn(2048))
		_, _ = r.Read(b)
		if i%2 == 0 && len(b) >= 48 {
			copy(b, "FILE")
			mftLE.PutUint16(b[20:], 48)
		}
		_, _ = mftDecodeRecord(b)
		_, _ = mftDataRuns(b)
		_, _ = mftListRecords(b)
		_, _ = mftParseRuns(b, 0)
		_ = mftFixup(b, 512)
	}
}
