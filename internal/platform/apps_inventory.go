package platform

// AppRecord contains native inventory facts, not cleanup or uninstall authority.
// RegistryRoot uses the stable HKLM/HKLM32/HKCU/SystemApp labels; core attaches
// rule plans separately so native readers never depend on the rule engine.
type AppRecord struct {
	ID, Name, Version, Publisher                                        string
	LastUsedDate, InstallDate                                           *string
	LastUsedRaw, InstallDateRaw                                         uint64
	InstallLocation, DisplayIcon, UninstallString, QuietUninstallString *string
	EstimatedSize                                                       uint64
	RegistryRoot, RegistrySubpath                                       string
	IsSystemComponent, UninstallerMissing                               bool
}
