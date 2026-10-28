package platform

import (
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

var residualCleanDeleteTree = windows.NewLazySystemDLL("advapi32.dll").NewProc("RegDeleteTreeW")

// WindowsResidualCleanAdapter owns native side effects. It never starts UAC or
// substitutes permanent deletion when the shell recycle operation fails.
type WindowsResidualCleanAdapter struct{ Context context.Context }

func (r WindowsResidualCleanAdapter) PendingRenameWords() []uint16 {
	if r.Context.Err() != nil {
		return nil
	}
	key, err := registry.OpenKey(registry.LOCAL_MACHINE, `SYSTEM\CurrentControlSet\Control\Session Manager`, registry.QUERY_VALUE)
	if err != nil {
		return nil
	}
	defer key.Close()
	const name = "PendingFileRenameOperations"
	n, ty, err := key.GetValue(name, nil)
	if err != nil || ty != registry.MULTI_SZ {
		return nil
	}
	data := make([]byte, n)
	n, ty, err = key.GetValue(name, data)
	if err != nil || ty != registry.MULTI_SZ {
		return nil
	}
	words := make([]uint16, n/2)
	for i := range words {
		words[i] = binary.LittleEndian.Uint16(data[i*2:])
	}
	return words
}

func (r WindowsResidualCleanAdapter) registryView(root string) (registry.Key, uint32, error) {
	if err := r.Context.Err(); err != nil {
		return 0, 0, err
	}
	switch root {
	case "HKLM":
		return registry.LOCAL_MACHINE, registry.WOW64_64KEY, nil
	case "HKLM32":
		return registry.LOCAL_MACHINE, registry.WOW64_32KEY, nil
	case "HKCU":
		if !residualCleanCurrentUser() {
			return 0, 0, errors.New("Startup user differs from current registry account; refusing HKCU mutation")
		}
		return registry.CURRENT_USER, 0, nil
	default:
		return 0, 0, errors.New("Unsupported residual registry root")
	}
}
func residualCleanCurrentUser() bool {
	startup, known := UserSID()
	current, currentKnown := fileopsProcessSID()
	return known && currentKnown && startup != "" && strings.EqualFold(startup, current)
}
func residualCleanRegistryAbsence(err error) (bool, bool) {
	if err == nil {
		return false, true
	}
	if errors.Is(err, windows.ERROR_FILE_NOT_FOUND) || errors.Is(err, windows.ERROR_PATH_NOT_FOUND) {
		return true, true
	}
	return false, false
}
func (r WindowsResidualCleanAdapter) RegistryAbsent(root, path, name string, value bool) (bool, bool) {
	if !residualCleanRegistryPath(path) {
		return false, false
	}
	h, view, err := r.registryView(root)
	if err != nil {
		return false, false
	}
	key, err := registry.OpenKey(h, path, view|registry.QUERY_VALUE)
	if err != nil {
		return residualCleanRegistryAbsence(err)
	}
	defer key.Close()
	if !value {
		return false, true
	}
	_, _, err = key.GetValue(name, nil)
	return residualCleanRegistryAbsence(err)
}
func (r WindowsResidualCleanAdapter) DeleteRegistry(root, path, name string, value bool) error {
	if !residualCleanRegistryPath(path) {
		return errors.New("Invalid residual registry path")
	}
	h, view, err := r.registryView(root)
	if err != nil {
		return err
	}
	if value {
		key, err := registry.OpenKey(h, path, view|registry.SET_VALUE)
		if err != nil {
			return err
		}
		defer key.Close()
		return key.DeleteValue(name)
	}
	parent, leaf := "", path
	if i := strings.LastIndexByte(path, '\\'); i >= 0 {
		parent, leaf = path[:i], path[i+1:]
	}
	key, err := registry.OpenKey(h, parent, view|windows.DELETE|registry.ENUMERATE_SUB_KEYS|registry.QUERY_VALUE|registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer key.Close()
	wide, err := windows.UTF16PtrFromString(leaf)
	if err != nil {
		return err
	}
	code, _, _ := residualCleanDeleteTree.Call(uintptr(key), uintptr(unsafe.Pointer(wide)))
	if code != 0 {
		return syscall.Errno(code)
	}
	return nil
}

func residualCleanTaskPath(name string) (string, error) {
	rel, valid := residualCleanTaskRelative(name)
	if !valid {
		return "", errors.New("Invalid residual task name")
	}
	root := os.Getenv("SystemRoot")
	if root == "" {
		root = `C:\Windows`
	}
	path := filepath.Join(root, "System32", "Tasks", rel)
	if err := appRejectStagingLinks(path); err != nil {
		return "", err
	}
	return path, nil
}
func (r WindowsResidualCleanAdapter) TaskAbsent(name string) (bool, bool) {
	if r.Context.Err() != nil {
		return false, false
	}
	path, err := residualCleanTaskPath(name)
	if err != nil {
		return false, false
	}
	_, err = os.Lstat(path)
	return residualCleanRegistryAbsence(err)
}
func (r WindowsResidualCleanAdapter) DeleteTask(name string) error {
	if err := r.Context.Err(); err != nil {
		return err
	}
	if _, err := residualCleanTaskPath(name); err != nil {
		return err
	}
	system, err := windows.GetSystemDirectory()
	if err != nil {
		return err
	}
	cmd := exec.Command(filepath.Join(system, "schtasks.exe"), "/Delete", "/TN", name, "/F")
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	result, known := RunCommandTreeContext(r.Context, cmd, 30*time.Second)
	// Exit status is diagnostic only; core uses the Tasks-file postcondition.
	if !known || !result.OK {
		return errors.New("Scheduled task command failed or timed out")
	}
	return nil
}

func (r WindowsResidualCleanAdapter) StopService(name string) error {
	if err := r.Context.Err(); err != nil {
		return err
	}
	if name == "" || strings.ContainsAny(name, "\\/\x00") {
		return errors.New("Invalid residual service name")
	}
	scm, err := windows.OpenSCManager(nil, nil, windows.SC_MANAGER_CONNECT)
	if err != nil {
		return err
	}
	defer windows.CloseServiceHandle(scm)
	wide, err := windows.UTF16PtrFromString(name)
	if err != nil {
		return err
	}
	service, err := windows.OpenService(scm, wide, windows.SERVICE_STOP|windows.SERVICE_QUERY_STATUS)
	if err != nil {
		return err
	}
	defer windows.CloseServiceHandle(service)
	var status windows.SERVICE_STATUS
	_ = windows.ControlService(service, windows.SERVICE_CONTROL_STOP, &status)
	for range 20 {
		if windows.QueryServiceStatus(service, &status) == nil && status.CurrentState == windows.SERVICE_STOPPED {
			return nil
		}
		timer := time.NewTimer(100 * time.Millisecond)
		select {
		case <-r.Context.Done():
			timer.Stop()
			return r.Context.Err()
		case <-timer.C:
		}
	}
	return errors.New("Service stop could not be confirmed")
}

func (r WindowsResidualCleanAdapter) TerminateUnder(roots []string, allowed func(string) bool) error {
	if err := r.Context.Err(); err != nil {
		return err
	}
	return appEnumerateProcesses(func(pid uint32, _ string) bool {
		if r.Context.Err() != nil {
			return true
		}
		if pid == 0 || pid == 4 || pid == uint32(os.Getpid()) {
			return false
		}
		process, err := windows.OpenProcess(windows.PROCESS_TERMINATE|windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
		if err != nil {
			return false
		}
		defer windows.CloseHandle(process)
		buffer := make([]uint16, 32768)
		size := uint32(len(buffer))
		if windows.QueryFullProcessImageName(process, 0, &buffer[0], &size) != nil {
			return false
		}
		image := windows.UTF16ToString(buffer[:size])
		if residualCleanProcessWithin(image, roots) && allowed(image) && r.Context.Err() == nil {
			_ = windows.TerminateProcess(process, 1)
		}
		return false
	})
}

func (r WindowsResidualCleanAdapter) MoveToTrash(path string) error {
	if err := r.Context.Err(); err != nil {
		return err
	}
	if !residualCleanCurrentUser() {
		return fmt.Errorf("Startup user differs from current shell account; refusing wrong-user recycle bin")
	}
	return MoveToTrash(path)
}
