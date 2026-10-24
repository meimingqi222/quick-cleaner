package platform

import (
	"fmt"
	"os/exec"
	"sync"
	"syscall"
	"unsafe"
)

var commandKernel = syscall.NewLazyDLL("kernel32.dll")
var createCommandJob = commandKernel.NewProc("CreateJobObjectW")
var setCommandJob = commandKernel.NewProc("SetInformationJobObject")
var assignCommandJob = commandKernel.NewProc("AssignProcessToJobObject")
var snapshotCommandThreads = commandKernel.NewProc("CreateToolhelp32Snapshot")
var firstCommandThread = commandKernel.NewProc("Thread32First")
var nextCommandThread = commandKernel.NewProc("Thread32Next")
var openCommandThread = commandKernel.NewProc("OpenThread")
var resumeCommandThread = commandKernel.NewProc("ResumeThread")

type commandJobBasic struct {
	ProcessTime, JobTime   int64
	Flags                  uint32
	MinWorking, MaxWorking uintptr
	ActiveProcesses        uint32
	Affinity               uintptr
	Priority, Scheduling   uint32
}
type commandJobLimits struct {
	Basic                                                      commandJobBasic
	IO                                                         [6]uint64
	ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory uintptr
}
type commandThreadEntry struct {
	Size, Usage, ID, Owner      uint32
	BasePriority, DeltaPriority int32
	Flags                       uint32
}

func resumeCommand(pid uint32) error {
	snap, _, e := snapshotCommandThreads.Call(4, 0)
	if snap == ^uintptr(0) {
		return e
	}
	defer syscall.CloseHandle(syscall.Handle(snap))
	entry := commandThreadEntry{Size: uint32(unsafe.Sizeof(commandThreadEntry{}))}
	ok, _, _ := firstCommandThread.Call(snap, uintptr(unsafe.Pointer(&entry)))
	for ok != 0 {
		if entry.Owner == pid {
			h, _, e := openCommandThread.Call(2, 0, uintptr(entry.ID))
			if h == 0 {
				return e
			}
			defer syscall.CloseHandle(syscall.Handle(h))
			n, _, e := resumeCommandThread.Call(h)
			if n != 1 {
				return fmt.Errorf("resume suspended child: count %d: %v", n, e)
			}
			return nil
		}
		ok, _, _ = nextCommandThread.Call(snap, uintptr(unsafe.Pointer(&entry)))
	}
	return fmt.Errorf("suspended child has no initial thread")
}

func startCommandTree(cmd *exec.Cmd) (func(), error) {
	job, _, e := createCommandJob.Call(0, 0)
	if job == 0 {
		return nil, e
	}
	var once sync.Once
	closeJob := func() { once.Do(func() { syscall.CloseHandle(syscall.Handle(job)) }) }
	limits := commandJobLimits{}
	limits.Basic.Flags = 0x2000 // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
	ok, _, e := setCommandJob.Call(job, 9, uintptr(unsafe.Pointer(&limits)), unsafe.Sizeof(limits))
	if ok == 0 {
		closeJob()
		return nil, e
	}
	if cmd.SysProcAttr == nil {
		cmd.SysProcAttr = &syscall.SysProcAttr{}
	}
	cmd.SysProcAttr.CreationFlags |= 0x08000004 // CREATE_NO_WINDOW | CREATE_SUSPENDED
	cmd.SysProcAttr.HideWindow = true
	if err := cmd.Start(); err != nil {
		closeJob()
		return nil, err
	}
	// exec.Cmd does not expose its process handle; reopen with the rights required
	// by AssignProcessToJobObject, while the sole initial thread is still suspended.
	h, err := syscall.OpenProcess(0x0100|0x0001, false, uint32(cmd.Process.Pid))
	if err == nil {
		ok, _, e = assignCommandJob.Call(job, uintptr(h))
		syscall.CloseHandle(h)
		if ok == 0 {
			err = e
		} else {
			err = resumeCommand(uint32(cmd.Process.Pid))
		}
	}
	if err != nil {
		_ = cmd.Process.Kill()
		closeJob()
		_ = cmd.Wait()
		return nil, err
	}
	return closeJob, nil
}
