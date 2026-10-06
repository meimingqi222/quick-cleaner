<#
.SYNOPSIS
  Reproducible shared-probe performance evidence for the rule engine (GOAL §4-G).

.DESCRIPTION
  Records the baseline (commit / workspace / toolchain / date), the fixture scale and
  the measurement method, then runs the production discovery over the isolated
  `rules/fixtures/*-extra.toml` fixtures N times and reports the discovery counters
  (enumeration / read / probe / manifest / candidate counts) and the wall clock.

  This proves that a scan performs a bounded, shared enumeration instead of repeated
  full-directory scans, and that the counters are identical across runs. It is a
  diagnostic only: `explain` builds an isolated plan and never deletes anything.

  The pre-migration implementation no longer exists in the tree, so a true
  before/after wall-clock comparison cannot be produced locally; this harness records
  the "after" side, its baseline and its method so the limitation is explicit.
#>
[CmdletBinding()]
param([int]$Runs = 5)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $repo
$exe = Join-Path $repo 'target/debug/examples/rules.exe'
$evidence = Join-Path $repo 'docs/agent-notes-evidence/2026-10-06-shared-probe-perf-baseline.json'
$humanlog = Join-Path $repo 'docs/agent-notes-evidence/2026-10-06-shared-probe-perf-baseline.log'

Write-Host 'building rules example...'
& cargo build --example rules | Out-Null
if (-not (Test-Path $exe)) { throw "missing $exe" }

function New-Root([string]$Tag) {
    $root = Join-Path ([IO.Path]::GetTempPath()) "qc_perf_$Tag"
    if (Test-Path $root) { Remove-Item -Recurse -Force $root }
    New-Item -ItemType Directory -Force -Path $root | Out-Null
    return $root
}
function MkDir([string]$p) { New-Item -ItemType Directory -Force -Path $p | Out-Null }
function Touch([string]$p) {
    MkDir (Split-Path $p)
    Set-Content -Path $p -Value 'x' -NoNewline
}

function Build-Directory([string]$root) {
    Touch (Join-Path $root 'Projects/isolated/project.marker')
    Touch (Join-Path $root 'Projects/isolated/preserved.data')
}
function Build-Cache([string]$root) {
    MkDir  (Join-Path $root '.cache/cache-tool')
    Touch  (Join-Path $root '.cache/random-app/Cache/data')
    Touch  (Join-Path $root '.cache/random-app/Code Cache/data')
    MkDir  (Join-Path $root '.cache/unknown-thing')
    Touch  (Join-Path $root '.cache/loose.txt')
}
function Build-Manifest([string]$root) {
    $posix = ($root -replace '\\', '/')
    MkDir (Join-Path $root 'vendor/extensions')
    Set-Content -Path (Join-Path $root 'vendor/extensions/.obsolete') -NoNewline `
        -Value '{"live.tool-1.0":true,"record.only-0.9":true,"unselected.tool-2.0":false,"../escape":true}'
    MkDir (Join-Path $root 'vendor/extensions/live.tool-1.0')
    foreach ($name in 'orphan', 'live', 'remote', 'encoded') {
        MkDir (Join-Path $root "editors/projects/$name")
    }
    Set-Content -NoNewline -Path (Join-Path $root 'editors/projects/orphan/workspace.json')  -Value ('{"folder":"file://' + $posix + '/gone-project"}')
    Set-Content -NoNewline -Path (Join-Path $root 'editors/projects/live/workspace.json')    -Value ('{"folder":"file://' + $posix + '/still-here"}')
    Set-Content -NoNewline -Path (Join-Path $root 'editors/projects/remote/workspace.json')  -Value '{"folder":"https://example.com/x"}'
    Set-Content -NoNewline -Path (Join-Path $root 'editors/projects/encoded/workspace.json') -Value ('{"folder":"file://' + $posix + '/my%20gone"}')
    MkDir (Join-Path $root 'still-here')
}

function Measure-Fixture([string]$Name, [string]$Toml, [string]$Root) {
    $samples = @()
    for ($i = 1; $i -le $Runs; $i++) {
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $out = & $exe explain 1 $Toml $Root
        $exit = $LASTEXITCODE
        $sw.Stop()
        $j = $out | ConvertFrom-Json
        $d = $j.directory_discovery
        $samples += [pscustomobject]@{
            run               = $i
            exit              = $exit
            wall_ms           = [int]$sw.ElapsedMilliseconds
            plans             = @($d.plans).Count
            directory_reads   = $d.directory_reads
            inventory_entries = $d.inventory_entries
            directory_probes  = @($d.directory_probes.PSObject.Properties).Count
            manifest_reads    = $d.manifest_reads
            manifest_rows     = $d.manifest_rows
            candidate_checks  = $d.candidate_checks
            budget_blocked    = $d.budget_blocked
        }
    }
    return [pscustomobject]@{ name = $Name; toml = $Toml; runs = $samples }
}

$dir     = New-Root 'dir';     Build-Directory $dir
$cache   = New-Root 'cache';   Build-Cache     $cache
$manifest = New-Root 'manifest'; Build-Manifest $manifest

$dirty = ((& git status --porcelain | Measure-Object).Count -gt 0)
$baseline = [pscustomobject]@{
    date          = (Get-Date -Format o)
    commit        = (& git rev-parse HEAD).Trim()
    branch        = (& git rev-parse --abbrev-ref HEAD).Trim()
    dirty         = $dirty
    rustc         = (& rustc --version).Trim()
    os            = [string]([Environment]::OSVersion.VersionString)
    rules_check   = (& $exe check)
    method        = "compiled rules.exe explain 1 <extra.toml> <root>, $Runs runs each; wall clock around the exe; counters from directory_discovery"
    fixture_scale = 'directory=1 marker (+sibling); cache=5 .cache children incl. Chromium leaves; manifest=.obsolete(4 rows)+4 workspace.json'
}

$results = @(
    Measure-Fixture 'directory-layout-extra' 'rules/fixtures/directory-layout-extra.toml' $dir
    Measure-Fixture 'cache-catalog-extra'    'rules/fixtures/cache-catalog-extra.toml'    $cache
    Measure-Fixture 'manifest-layout-extra'  'rules/fixtures/manifest-layout-extra.toml'  $manifest
)

[pscustomobject]@{ baseline = $baseline; results = $results } |
    ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 $evidence

$lines = New-Object System.Collections.Generic.List[string]
$lines.Add("shared-probe performance evidence (GOAL 4-G)")
$lines.Add("date: $($baseline.date)")
$lines.Add("commit: $($baseline.commit) ($($baseline.branch)) dirty=$($baseline.dirty)")
$lines.Add("rustc: $($baseline.rustc)")
$lines.Add("os: $($baseline.os)")
$lines.Add("rules: $($baseline.rules_check)")
$lines.Add("method: $($baseline.method)")
$lines.Add("fixture scale: $($baseline.fixture_scale)")
$lines.Add("")
foreach ($r in $results) {
    $walls = ($r.runs | ForEach-Object { $_.wall_ms })
    $reads = ($r.runs | ForEach-Object { $_.directory_reads } | Sort-Object -Unique)
    $cand  = ($r.runs | ForEach-Object { $_.candidate_checks } | Sort-Object -Unique)
    $lines.Add("$($r.name):")
    $lines.Add("  wall_ms runs: [$($walls -join ', ')]")
    $lines.Add("  directory_reads distinct: [$($reads -join ', ')]  candidate_checks distinct: [$($cand -join ', ')]")
    foreach ($run in $r.runs) {
        $lines.Add("  " + ($run | ConvertTo-Json -Compress))
    }
}
$lines | Set-Content -Encoding utf8 $humanlog

Write-Host "wrote $evidence"
Write-Host "wrote $humanlog"
$lines | ForEach-Object { Write-Host $_ }
