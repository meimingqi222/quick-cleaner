package platform

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"unsafe"

	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

var appCreateWithToken = windows.NewLazySystemDLL("advapi32.dll").NewProc("CreateProcessWithTokenW")

// RunUninstallShell waits for the leader only. Official installers may relaunch
// outside their original tree; core owns settlement, not a kill-on-close Job.
func RunUninstallShell(cmdline string, hidden, unelevated bool) (uint32, error) {
	system, err := windows.GetSystemDirectory()
	if err != nil {
		return 0, err
	}
	app := filepath.Join(system, "cmd.exe")
	line := QuoteWindowsArgument(app) + " /c " + cmdline
	return appRunNative(app, line, "", hidden, unelevated, false)
}

// RunUninstallExecutable preserves a registered vendor command's current
// permissions and window visibility; only package-manager shell routes demote.
func RunUninstallExecutable(executable string, args []string) (uint32, error) {
	return appRunNative(executable, appExecutableLine(executable, args), "", false, false, false)
}

// RunOfficialAppCommand uses a reviewed source adapter's literal argv and cwd,
// refusing to run as another account or to fall back to administrator rights.
func RunOfficialAppCommand(executable string, args []string, cwd string) (uint32, error) {
	return appRunNative(executable, appExecutableLine(executable, args), cwd, true, true, true)
}

func appRunNative(app, line, cwd string, hidden, desktop, verifyCurrent bool) (uint32, error) {
	if !appNativeCommandValid(app, line) {
		return 0, fmt.Errorf("invalid native command line")
	}
	current, err := windows.OpenCurrentProcessToken()
	if err != nil {
		return 0, err
	}
	defer current.Close()
	var token windows.Token
	if desktop && current.IsElevated() {
		appEnableImpersonate()
		token, err = appDesktopToken(current)
		if err != nil {
			return 0, err
		}
		defer token.Close()
	} else if verifyCurrent {
		if !appExpectedSID(current) {
			return 0, fmt.Errorf("uninstall user SID mismatch")
		}
	}
	if token != 0 {
		cache, _ := UserCacheDir()
		system, err := windows.GetSystemDirectory()
		if err != nil {
			return 0, err
		}
		prepared, err := prepareAppTokenCommand(app, line, cwd, cache, filepath.Join(system, "WindowsPowerShell", "v1.0", "powershell.exe"), appRejectStagingLinks)
		if err != nil {
			return 0, err
		}
		defer prepared.cleanup()
		app, line, cwd = prepared.application, prepared.line, prepared.cwd
	}
	application, err := windows.UTF16PtrFromString(app)
	if err != nil {
		return 0, err
	}
	command, err := windows.UTF16FromString(line)
	if err != nil {
		return 0, err
	}
	var directory *uint16
	if cwd != "" {
		directory, err = windows.UTF16PtrFromString(cwd)
		if err != nil {
			return 0, err
		}
	}
	si := windows.StartupInfo{Cb: uint32(unsafe.Sizeof(windows.StartupInfo{}))}
	flags := uint32(windows.CREATE_UNICODE_ENVIRONMENT)
	if hidden {
		flags |= windows.CREATE_NO_WINDOW
		si.Flags = windows.STARTF_USESHOWWINDOW
		si.ShowWindow = windows.SW_HIDE
	}
	var pi windows.ProcessInformation
	if token != 0 {
		var env *uint16
		if err = windows.CreateEnvironmentBlock(&env, token, false); err != nil {
			return 0, fmt.Errorf("desktop-user environment: %w", err)
		}
		// LazyProc.Call saves GetLastError before any environment cleanup API.
		ok, _, launchErr := appCreateWithToken.Call(uintptr(token), 0, uintptr(unsafe.Pointer(application)), uintptr(unsafe.Pointer(&command[0])), uintptr(flags), uintptr(unsafe.Pointer(env)), uintptr(unsafe.Pointer(directory)), uintptr(unsafe.Pointer(&si)), uintptr(unsafe.Pointer(&pi)))
		_ = windows.DestroyEnvironmentBlock(env)
		if ok == 0 {
			return 0, fmt.Errorf("desktop-user launch: %w", launchErr)
		}
	} else {
		if err = windows.CreateProcess(application, &command[0], nil, nil, false, flags, nil, directory, &si, &pi); err != nil {
			return 0, err
		}
	}
	windows.CloseHandle(pi.Thread)
	defer windows.CloseHandle(pi.Process)
	result, err := windows.WaitForSingleObject(pi.Process, windows.INFINITE)
	if err != nil {
		return 0, err
	}
	if result != windows.WAIT_OBJECT_0 {
		return 0, fmt.Errorf("unexpected uninstall wait result: %d", result)
	}
	var code uint32
	err = windows.GetExitCodeProcess(pi.Process, &code)
	return code, err
}

func appExpectedSID(token windows.Token) bool {
	expected, ok := UserSID()
	if !ok || expected == "" {
		return false
	}
	u, err := token.GetTokenUser()
	return err == nil && u.User.Sid.String() == expected
}

func appVerifiedDesktopToken(token windows.Token, session uint32) bool {
	var actual, size uint32
	return appExpectedSID(token) && !token.IsElevated() && windows.GetTokenInformation(token, windows.TokenSessionId, (*byte)(unsafe.Pointer(&actual)), 4, &size) == nil && actual == session
}

func appDuplicatePrimary(token windows.Token) (windows.Token, error) {
	var primary windows.Token
	err := windows.DuplicateTokenEx(token, windows.TOKEN_QUERY|windows.TOKEN_DUPLICATE|windows.TOKEN_ASSIGN_PRIMARY|windows.TOKEN_ADJUST_DEFAULT|windows.TOKEN_ADJUST_SESSIONID, nil, windows.SecurityImpersonation, windows.TokenPrimary, &primary)
	return primary, err
}

func appDesktopToken(current windows.Token) (windows.Token, error) {
	var session uint32
	if err := windows.ProcessIdToSessionId(windows.GetCurrentProcessId(), &session); err != nil {
		return 0, err
	}
	var found windows.Token
	appEnumerateProcesses(func(pid uint32, name string) bool {
		if !strings.EqualFold(name, "explorer.exe") {
			return false
		}
		var candidateSession uint32
		if windows.ProcessIdToSessionId(pid, &candidateSession) != nil || candidateSession != session {
			return false
		}
		p, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
		if err != nil {
			return false
		}
		defer windows.CloseHandle(p)
		var token windows.Token
		if windows.OpenProcessToken(p, windows.TOKEN_QUERY|windows.TOKEN_DUPLICATE, &token) != nil {
			return false
		}
		defer token.Close()
		if !appVerifiedDesktopToken(token, session) {
			return false
		}
		found, _ = appDuplicatePrimary(token)
		return found != 0
	})
	if found != 0 {
		return found, nil
	}
	linked, err := current.GetLinkedToken()
	if err == nil {
		defer linked.Close()
		if appVerifiedDesktopToken(linked, session) {
			return appDuplicatePrimary(linked)
		}
	}
	return 0, fmt.Errorf("no verified desktop-user token in current session")
}

func appEnableImpersonate() {
	var token windows.Token
	if windows.OpenProcessToken(windows.CurrentProcess(), windows.TOKEN_ADJUST_PRIVILEGES|windows.TOKEN_QUERY, &token) != nil {
		return
	}
	defer token.Close()
	name, _ := windows.UTF16PtrFromString("SeImpersonatePrivilege")
	var luid windows.LUID
	if windows.LookupPrivilegeValue(nil, name, &luid) != nil {
		return
	}
	tp := windows.Tokenprivileges{PrivilegeCount: 1}
	tp.Privileges[0] = windows.LUIDAndAttributes{Luid: luid, Attributes: windows.SE_PRIVILEGE_ENABLED}
	_ = windows.AdjustTokenPrivileges(token, false, &tp, 0, nil, nil)
}

func appRejectStagingLinks(path string) error {
	for {
		info, err := os.Lstat(path)
		if err != nil && !os.IsNotExist(err) {
			return err
		}
		if err == nil {
			p, e := windows.UTF16PtrFromString(path)
			if e != nil {
				return e
			}
			attr, e := windows.GetFileAttributes(p)
			if e != nil {
				return e
			}
			if info.Mode()&os.ModeSymlink != 0 || attr&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
				return fmt.Errorf("linked command staging directory")
			}
		}
		parent := filepath.Dir(path)
		if parent == path {
			return nil
		}
		path = parent
	}
}

func appEnumerateProcesses(visit func(uint32, string) bool) error {
	snapshot, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPPROCESS, 0)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(snapshot)
	entry := windows.ProcessEntry32{Size: uint32(unsafe.Sizeof(windows.ProcessEntry32{}))}
	for err = windows.Process32First(snapshot, &entry); err == nil; err = windows.Process32Next(snapshot, &entry) {
		if visit(entry.ProcessID, windows.UTF16ToString(entry.ExeFile[:])) {
			return nil
		}
	}
	if errors.Is(err, windows.ERROR_NO_MORE_FILES) {
		return nil
	}
	return err
}

func HasRelatedAppProcess(installDir, uninstallerStem string) bool {
	found := false
	err := appEnumerateProcesses(func(pid uint32, name string) bool {
		image := ""
		if installDir != "" {
			if p, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid); err == nil {
				buf := make([]uint16, 32768)
				size := uint32(len(buf))
				if windows.QueryFullProcessImageName(p, 0, &buf[0], &size) == nil {
					image = windows.UTF16ToString(buf[:size])
				}
				windows.CloseHandle(p)
			}
		}
		found = appRelatedProcess(image, name, installDir, uninstallerStem)
		return found
	})
	// Failed observation must not authorize residual cleanup while an installer
	// may still be running.
	return err != nil || found
}

func ReadWindowsAppProcesses() ([]AppRunningProcess, error) {
	var out []AppRunningProcess
	err := appEnumerateProcesses(func(pid uint32, name string) bool {
		image := ""
		if p, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid); err == nil {
			buf := make([]uint16, 32768)
			size := uint32(len(buf))
			if windows.QueryFullProcessImageName(p, 0, &buf[0], &size) == nil {
				image = windows.UTF16ToString(buf[:size])
			}
			windows.CloseHandle(p)
		}
		out = append(out, AppRunningProcess{PID: pid, Name: name, Image: image})
		return false
	})
	return out, err
}

func AppRegistrationPresent(rootLabel, subpath string) bool {
	var root registry.Key
	access := uint32(registry.READ)
	switch rootLabel {
	case "HKLM":
		root = registry.LOCAL_MACHINE
		access |= registry.WOW64_64KEY
	case "HKLM32":
		root = registry.LOCAL_MACHINE
		access |= registry.WOW64_32KEY
	case "HKCU":
		root = registry.CURRENT_USER
	default:
		return false
	}
	key, err := registry.OpenKey(root, subpath, access|registry.QUERY_VALUE)
	if err != nil {
		return !errors.Is(err, windows.ERROR_FILE_NOT_FOUND) && !errors.Is(err, windows.ERROR_PATH_NOT_FOUND)
	}
	key.Close()
	return true
}
