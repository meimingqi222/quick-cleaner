package core

type PathOperation string
type DirectoryRoot string
type VersionRoot string
type VersionPlatform string
type SourceAction string
type SourceCompletion string
type RuleSafety string
type RuleGroup string
type ResidualSource string
type Shell string

type RuleBundle struct {
	Schema   uint32           `toml:"schema" json:"schema"`
	Sequence uint64           `toml:"sequence" json:"sequence"`
	Rules    []RuleDefinition `toml:"rules" json:"rules"`
}
type RuleDefinition struct {
	ID                    string                        `toml:"id" json:"id"`
	Version               uint32                        `toml:"version" json:"version"`
	Platform              string                        `toml:"platform" json:"platform"`
	Required              []string                      `toml:"required" json:"required"`
	Lists                 map[string][]string           `toml:"lists" json:"lists"`
	Numbers               map[string]uint64             `toml:"numbers" json:"numbers"`
	ProviderPolicies      map[string]ProviderPolicy     `toml:"provider_policies" json:"provider_policies"`
	Categories            map[string]CategoryPolicy     `toml:"categories" json:"categories"`
	Entries               []PathRule                    `toml:"entries" json:"entries"`
	Directories           []DirectoryRule               `toml:"directories" json:"directories"`
	VersionLayouts        []VersionLayout               `toml:"version_layouts" json:"version_layouts"`
	Markers               []BuildMarker                 `toml:"markers" json:"markers"`
	App                   *SourceInstallRule            `toml:"app" json:"app"`
	Facts                 map[string]Probe              `toml:"facts" json:"facts"`
	Variables             map[string]Variable           `toml:"variables" json:"variables"`
	Detect                *Condition                    `toml:"detect" json:"detect"`
	Preserve              []string                      `toml:"preserve" json:"preserve"`
	Catalogs              map[string][]Layout           `toml:"catalogs" json:"catalogs"`
	Locations             map[string][]ResidualLocation `toml:"locations" json:"locations"`
	AppDataAliases        []AppDataAlias                `toml:"app_data_aliases" json:"app_data_aliases"`
	ScriptUninstallers    []ScriptUninstaller           `toml:"script_uninstallers" json:"script_uninstallers"`
	UninstallDataWarnings []RegisteredUninstallWarning  `toml:"uninstall_data_warnings" json:"uninstall_data_warnings"`
}
type ProviderPolicy struct {
	Recommended bool     `toml:"recommended" json:"recommended"`
	Disposal    Disposal `toml:"disposal" json:"disposal"`
}
type CategoryPolicy struct {
	Safety RuleSafety `toml:"safety" json:"safety"`
	Group  RuleGroup  `toml:"group" json:"group"`
}
type PathRule struct {
	Root              string        `toml:"root" json:"root"`
	Path              string        `toml:"path" json:"path"`
	Zh                string        `toml:"zh" json:"zh"`
	En                string        `toml:"en" json:"en"`
	Category          string        `toml:"category" json:"category"`
	Recommended       bool          `toml:"recommended" json:"recommended"`
	Operation         PathOperation `toml:"operation" json:"operation"`
	Disposal          Disposal      `toml:"disposal" json:"disposal"`
	MinimumAgeSeconds uint64        `toml:"minimum_age_seconds" json:"minimum_age_seconds"`
}
type Layout struct {
	Path     string        `toml:"path" json:"path"`
	Zh       string        `toml:"zh" json:"zh"`
	En       string        `toml:"en" json:"en"`
	Children []LayoutChild `toml:"children" json:"children"`
}
type LayoutChild struct {
	Path        string `toml:"path" json:"path"`
	Recommended bool   `toml:"recommended" json:"recommended"`
}
type ResidualLocation struct {
	Path   string         `toml:"path" json:"path"`
	Source ResidualSource `toml:"source" json:"source"`
}
type AppDataAlias struct {
	RegistryID string `toml:"registry_id" json:"registry_id"`
	Name       string `toml:"name" json:"name"`
	Publisher  string `toml:"publisher" json:"publisher"`
	Directory  string `toml:"directory" json:"directory"`
}
type BuildMarker struct {
	Dir        string   `toml:"dir" json:"dir"`
	Zh         string   `toml:"zh" json:"zh"`
	En         string   `toml:"en" json:"en"`
	SiblingAny []string `toml:"sibling_any" json:"sibling_any"`
}
type DirectoryRule struct {
	ID          string        `toml:"id" json:"id"`
	Root        DirectoryRoot `toml:"root" json:"root"`
	Paths       []string      `toml:"paths" json:"paths"`
	Select      Selection     `toml:"select" json:"select"`
	Zh          string        `toml:"zh" json:"zh"`
	En          string        `toml:"en" json:"en"`
	Category    string        `toml:"category" json:"category"`
	Operation   PathOperation `toml:"operation" json:"operation"`
	Disposal    Disposal      `toml:"disposal" json:"disposal"`
	Recommended bool          `toml:"recommended" json:"recommended"`
	LogPolicy   *LogPolicy    `toml:"log_policy" json:"log_policy"`
}

// Selection is a tagged declaration, not an executable callback. Validation rejects unknown tags.
type Selection struct {
	Kind              string                   `toml:"kind" json:"kind"`
	DirectoriesOnly   bool                     `toml:"directories_only" json:"directories_only"`
	Prefixes          []string                 `toml:"prefixes" json:"prefixes"`
	Suffixes          []string                 `toml:"suffixes" json:"suffixes"`
	Paths             []string                 `toml:"paths" json:"paths"`
	ExcludeNamePolicy string                   `toml:"exclude_name_policy" json:"exclude_name_policy"`
	Name              string                   `toml:"name" json:"name"`
	ChildDepth        uint8                    `toml:"child_depth" json:"child_depth"`
	Manifest          string                   `toml:"manifest" json:"manifest"`
	SelectedValue     bool                     `toml:"selected_value" json:"selected_value"`
	Pointer           string                   `toml:"pointer" json:"pointer"`
	URIPrefix         string                   `toml:"uri_prefix" json:"uri_prefix"`
	Scope             DirectoryRoot            `toml:"scope" json:"scope"`
	CatalogPolicy     map[string]CatalogPolicy `toml:"catalog_policy" json:"catalog_policy"`
	LeafPolicy        *LeafPolicy              `toml:"leaf_policy" json:"leaf_policy"`
	Catalog           string                   `toml:"catalog" json:"catalog"`
	Signature         string                   `toml:"signature" json:"signature"`
	Containers        []string                 `toml:"containers" json:"containers"`
}
type CatalogPolicy struct {
	Category    string `toml:"category" json:"category"`
	Recommended bool   `toml:"recommended" json:"recommended"`
}
type LeafPolicy struct {
	Signature string `toml:"signature" json:"signature"`
	Category  string `toml:"category" json:"category"`
	Zh        string `toml:"zh" json:"zh"`
	En        string `toml:"en" json:"en"`
}
type LogPolicy struct {
	Extensions      []string `toml:"extensions" json:"extensions"`
	ExcludeExact    []string `toml:"exclude_exact" json:"exclude_exact"`
	ExcludePrefixes []string `toml:"exclude_prefixes" json:"exclude_prefixes"`
}
type VersionLayout struct {
	ID          string          `toml:"id" json:"id"`
	Root        VersionRoot     `toml:"root" json:"root"`
	Platform    VersionPlatform `toml:"platform" json:"platform"`
	Path        string          `toml:"path" json:"path"`
	Versions    string          `toml:"versions" json:"versions"`
	Current     *string         `toml:"current" json:"current"`
	Lock        *LockPolicy     `toml:"lock" json:"lock"`
	Leaf        *string         `toml:"leaf" json:"leaf"`
	Prefixes    []string        `toml:"prefixes" json:"prefixes"`
	Exclude     []string        `toml:"exclude" json:"exclude"`
	Zh          string          `toml:"zh" json:"zh"`
	En          string          `toml:"en" json:"en"`
	Category    string          `toml:"category" json:"category"`
	Operation   PathOperation   `toml:"operation" json:"operation"`
	Disposal    Disposal        `toml:"disposal" json:"disposal"`
	Recommended bool            `toml:"recommended" json:"recommended"`
	Downloads   *DownloadPolicy `toml:"downloads" json:"downloads"`
}
type LockPolicy struct {
	Path              string `toml:"path" json:"path"`
	MaximumAgeSeconds uint64 `toml:"maximum_age_seconds" json:"maximum_age_seconds"`
}
type DownloadPolicy struct {
	Path        string        `toml:"path" json:"path"`
	Suffixes    []string      `toml:"suffixes" json:"suffixes"`
	Zh          string        `toml:"zh" json:"zh"`
	En          string        `toml:"en" json:"en"`
	Operation   PathOperation `toml:"operation" json:"operation"`
	Disposal    Disposal      `toml:"disposal" json:"disposal"`
	Recommended bool          `toml:"recommended" json:"recommended"`
}
type ScriptUninstaller struct {
	BundleID          string   `toml:"bundle_id" json:"bundle_id"`
	Script            string   `toml:"script" json:"script"`
	SHA256            []string `toml:"sha256" json:"sha256"`
	Interpreter       Shell    `toml:"interpreter" json:"interpreter"`
	Arguments         []string `toml:"arguments" json:"arguments"`
	PreservesUserData bool     `toml:"preserves_user_data" json:"preserves_user_data"`
	UserBundles       []string `toml:"user_bundles" json:"user_bundles"`
	SystemBundles     []string `toml:"system_bundles" json:"system_bundles"`
}
type RegisteredUninstallWarning struct {
	RegistryID string `toml:"registry_id" json:"registry_id"`
	Name       string `toml:"name" json:"name"`
	Publisher  string `toml:"publisher" json:"publisher"`
}
type SourceInstallRule struct {
	Actions              []SourceAction   `toml:"actions" json:"actions"`
	Completion           SourceCompletion `toml:"completion" json:"completion"`
	Name                 string           `toml:"name" json:"name"`
	DefaultHome          string           `toml:"default_home" json:"default_home"`
	Source               string           `toml:"source" json:"source"`
	Targets              []string         `toml:"targets" json:"targets"`
	SourceFile           string           `toml:"source_file" json:"source_file"`
	SourceSignature      string           `toml:"source_signature" json:"source_signature"`
	Constants            string           `toml:"constants" json:"constants"`
	SourceOwner          string           `toml:"source_owner" json:"source_owner"`
	Stamp                string           `toml:"stamp" json:"stamp"`
	VersionPointer       string           `toml:"version_pointer" json:"version_pointer"`
	StateDir             string           `toml:"state_dir" json:"state_dir"`
	StateFacts           string           `toml:"state_facts" json:"state_facts"`
	EnvironmentPointer   string           `toml:"environment_pointer" json:"environment_pointer"`
	Environments         string           `toml:"environments" json:"environments"`
	EnvironmentName      string           `toml:"environment_name" json:"environment_name"`
	ToolsDir             string           `toml:"tools_dir" json:"tools_dir"`
	ToolFacts            string           `toml:"tool_facts" json:"tool_facts"`
	ToolLock             string           `toml:"tool_lock" json:"tool_lock"`
	Launchers            []string         `toml:"launchers" json:"launchers"`
	Profiles             string           `toml:"profiles" json:"profiles"`
	Module               string           `toml:"module" json:"module"`
	ModuleFile           string           `toml:"module_file" json:"module_file"`
	ModuleArgs           []string         `toml:"module_args" json:"module_args"`
	HomeVariable         string           `toml:"home_variable" json:"home_variable"`
	PythonPrefix         string           `toml:"python_prefix" json:"python_prefix"`
	Installer            string           `toml:"installer" json:"installer"`
	InstallerSignature   string           `toml:"installer_signature" json:"installer_signature"`
	BootstrapDir         string           `toml:"bootstrap_dir" json:"bootstrap_dir"`
	BootstrapPrefix      string           `toml:"bootstrap_prefix" json:"bootstrap_prefix"`
	BootstrapExtensions  []string         `toml:"bootstrap_extensions" json:"bootstrap_extensions"`
	GatewayDir           string           `toml:"gateway_dir" json:"gateway_dir"`
	GatewayFiles         []string         `toml:"gateway_files" json:"gateway_files"`
	TaskPrefix           string           `toml:"task_prefix" json:"task_prefix"`
	PathPrefixes         []string         `toml:"path_prefixes" json:"path_prefixes"`
	EnvironmentVariables []string         `toml:"environment_variables" json:"environment_variables"`
	Preserve             []string         `toml:"preserve" json:"preserve"`
	InstallerProcess     string           `toml:"installer_process" json:"installer_process"`
	UnknownProcesses     []string         `toml:"unknown_processes" json:"unknown_processes"`
}
