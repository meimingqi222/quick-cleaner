package core

import (
	"fmt"
	"strings"
)

func sourceRegistrationScript(home, environmentKey string, exclusive, tasks bool, rule SourceInstallRule) string {
	literal := func(value string) string { return "'" + strings.ReplaceAll(value, "'", "''") + "'" }
	array := func(values []string) string {
		out := make([]string, len(values))
		for i, value := range values {
			out[i] = literal(value)
		}
		return "@(" + strings.Join(out, ",") + ")"
	}
	boolean := func(value bool) string {
		if value {
			return "$true"
		}
		return "$false"
	}
	return fmt.Sprintf("$installHome = %s; $environmentKey = %s; $exclusive = %s; $checkTasks = %s;\n$taskPrefix=%s; $gatewayDir=%s; $gatewayFiles=%s; $pathNames=%s; $sourceName=%s; $environmentNames=%s;\n", literal(home), literal(environmentKey), boolean(exclusive), boolean(tasks), literal(rule.TaskPrefix), literal(rule.GatewayDir), array(rule.GatewayFiles), array(rule.PathPrefixes), literal(rule.Source), array(rule.EnvironmentVariables)) + sourceRegistrationBody
}

const sourceRegistrationBody = `
$ErrorActionPreference = 'Stop'
function Under([string]$value, [string]$root) {
    $value = [Environment]::ExpandEnvironmentVariables($value.Trim().Trim('"')).Replace('/', '\').TrimEnd('\')
    $root = $root.Replace('/', '\').TrimEnd('\')
    return $value.Equals($root, [StringComparison]::OrdinalIgnoreCase) -or $value.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)
}
try {
    if ($checkTasks) {
        $service = New-Object -ComObject 'Schedule.Service'; $service.Connect()
        if ($exclusive) {
            $folder = $service.GetFolder('\')
            foreach ($task in @($folder.GetTasks(1))) {
                if ($task.Name -notlike ($taskPrefix + '*')) { continue }
                $xml = [xml]$task.Xml; $owned = $false
                foreach ($action in $xml.Task.Actions.Exec) {
                    $text = ([string]$action.Command + ' ' + [string]$action.Arguments).Replace('/', '\')
                    if ($text.IndexOf($installHome.TrimEnd('\') + '\' + $gatewayDir + '\', [StringComparison]::OrdinalIgnoreCase) -ge 0) { $owned = $true }
                }
                if ($owned) {
                    if ($task.GetInstances(0).Count -gt 0) { $task.Stop(0) }
                    $folder.DeleteTask($task.Name, 0)
                    if (@($folder.GetTasks(1) | Where-Object { $_.Name -eq $task.Name }).Count -gt 0) { throw 'Gateway task remains' }
                }
            }
        }
        $startup = [Environment]::GetFolderPath('Startup')
        foreach ($name in $gatewayFiles) {
            $entry = Join-Path $startup $name
            if ($exclusive -and (Test-Path -LiteralPath $entry)) {
                $text = [IO.File]::ReadAllText($entry).Replace('/', '\')
                if ($text.IndexOf($installHome.TrimEnd('\') + '\' + $gatewayDir + '\', [StringComparison]::OrdinalIgnoreCase) -ge 0) { throw 'Gateway Startup entry remains' }
            }
        }
    }
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($environmentKey, $true)
    if ($null -ne $key) {
        try {
            $names = @($sourceName); if ($exclusive) { $names = $pathNames }
            $prefixes = $names | ForEach-Object { Join-Path $installHome $_ }
            $pathValue = $key.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            if ($null -ne $pathValue) {
                $kept = @(([string]$pathValue).Split(';') | Where-Object {
                    $entry = $_; -not (@($prefixes | Where-Object { Under $entry $_ }).Count -gt 0)
                })
                $key.SetValue('Path', ($kept -join ';'), $key.GetValueKind('Path'))
                if (@(([string]$key.GetValue('Path')).Split(';') | Where-Object {
                    $entry = $_; @($prefixes | Where-Object { Under $entry $_ }).Count -gt 0
                }).Count -gt 0) { throw 'Hermes PATH remains' }
            }
            foreach ($name in $environmentNames) {
                $value = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                if ($exclusive -and $null -ne $value -and (Under ([string]$value) $installHome)) { $key.DeleteValue($name) }
            }
        } finally { $key.Dispose() }
    }
    exit 0
} catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }
`
