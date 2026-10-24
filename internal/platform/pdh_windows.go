package platform

import (
	"runtime"
	"syscall"
	"unicode/utf16"
	"unsafe"
)

var telemetryPDH = syscall.NewLazyDLL("pdh.dll")
var pdhOpen = telemetryPDH.NewProc("PdhOpenQueryW")
var pdhAddEnglish = telemetryPDH.NewProc("PdhAddEnglishCounterW")
var pdhCollect = telemetryPDH.NewProc("PdhCollectQueryData")
var pdhArray = telemetryPDH.NewProc("PdhGetFormattedCounterArrayW")
var pdhClose = telemetryPDH.NewProc("PdhCloseQuery")

// Callers serialize access and retain the query between samples: rate counters
// need two collections on the same handle, not a freshly opened query per tick.
type pdhQuery struct {
	handle   uintptr
	counters []uintptr
}

func openPDHQuery(paths []string) *pdhQuery {
	q := &pdhQuery{}
	if status, _, _ := pdhOpen.Call(0, 0, uintptr(unsafe.Pointer(&q.handle))); status != 0 {
		return nil
	}
	for _, path := range paths {
		wide, err := syscall.UTF16PtrFromString(path)
		if err != nil {
			q.close()
			return nil
		}
		var counter uintptr
		status, _, _ := pdhAddEnglish.Call(q.handle, uintptr(unsafe.Pointer(wide)), 0, uintptr(unsafe.Pointer(&counter)))
		if status != 0 {
			q.close()
			return nil
		}
		q.counters = append(q.counters, counter)
	}
	return q
}

func (q *pdhQuery) close() {
	_, _, _ = pdhClose.Call(q.handle)
	q.handle = 0
}

func (q *pdhQuery) collect() bool {
	status, _, _ := pdhCollect.Call(q.handle)
	return status == 0
}

type pdhCounterItem struct {
	Name   *uint16
	Status uint32
	Value  float64
}

func (q *pdhQuery) values(index int) map[string]float64 {
	out := make(map[string]float64)
	if index < 0 || index >= len(q.counters) {
		return out
	}
	const formatDouble = 0x200
	const moreData = 0x800007d2
	var size, count uint32
	status, _, _ := pdhArray.Call(q.counters[index], formatDouble, uintptr(unsafe.Pointer(&size)), uintptr(unsafe.Pointer(&count)), 0)
	if uint32(status) != moreData || size == 0 {
		return out
	}
	// The byte count includes strings after the item array. uint64 storage
	// provides the native union's alignment, unlike an arbitrary byte buffer.
	buffer := make([]uint64, (uint64(size)+7)/8)
	allocated := uint64(len(buffer)) * 8
	status, _, _ = pdhArray.Call(q.counters[index], formatDouble, uintptr(unsafe.Pointer(&size)), uintptr(unsafe.Pointer(&count)), uintptr(unsafe.Pointer(&buffer[0])))
	itemSize := uint64(unsafe.Sizeof(pdhCounterItem{}))
	if status != 0 || uint64(size) > allocated || uint64(count)*itemSize > uint64(size) {
		return out
	}
	items := unsafe.Slice((*pdhCounterItem)(unsafe.Pointer(&buffer[0])), int(count))
	base := uintptr(unsafe.Pointer(&buffer[0]))
	for _, item := range items {
		if item.Status > 1 || item.Name == nil {
			continue
		}
		offset := uintptr(unsafe.Pointer(item.Name)) - base
		if offset >= uintptr(size) || offset%2 != 0 {
			continue
		}
		wide := unsafe.Slice(item.Name, (uintptr(size)-offset)/2)
		for i, c := range wide {
			if c == 0 {
				out[string(utf16.Decode(wide[:i]))] += item.Value
				break
			}
		}
	}
	runtime.KeepAlive(buffer)
	return out
}
