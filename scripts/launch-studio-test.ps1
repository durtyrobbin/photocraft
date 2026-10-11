param([switch]$SafeGpu, [switch]$CheckOnly, [string]$ProfileRoot)
$ErrorActionPreference = 'Stop'
$studioRepo = Split-Path -Parent $PSScriptRoot
$studioExe = Join-Path $studioRepo 'target\release\photocraft.exe'
if (!$ProfileRoot) { $ProfileRoot = Join-Path $studioRepo 'target\studio-test-profile' }
$studioProfile = [IO.Path]::GetFullPath($ProfileRoot)
# This local Windows test launcher follows the owner's E:-storage policy.
foreach ($studioPath in @($studioRepo, $studioProfile)) {
    if (!$studioPath.StartsWith('E:\PROJECTS\AIOS\', [StringComparison]::OrdinalIgnoreCase)) {
        throw "Keep this local test checkout and profile under E:\PROJECTS\AIOS: $studioPath"
    }
}
$studioSample = Join-Path $studioRepo 'docs\studio-evidence\loaded-paint\synthetic-source.png'
$studioPresetSource = Join-Path $studioRepo 'presets\studio-loaded-paint\Presets'
foreach ($studioFile in @($studioExe, $studioSample, (Join-Path $studioPresetSource 'index.json'))) {
    if (!(Test-Path -LiteralPath $studioFile -PathType Leaf)) { throw "Required test file is missing: $studioFile. See docs/studio-fast-test.md." }
}
if ($CheckOnly) { Write-Output 'Optimized executable, sample and preset package found; no files changed.'; return }
$studioSettings = Join-Path $studioProfile 'settings'
$studioTemp = Join-Path $studioProfile 'temp'
$studioPaintings = Join-Path $studioProfile 'paintings'
foreach ($studioDirectory in @($studioSettings, $studioTemp, $studioPaintings)) {
    New-Item -ItemType Directory -Path $studioDirectory -Force | Out-Null
}
# Seed only a new profile. Never overwrite an existing brush library or preferences.
$studioPresetTarget = Join-Path $studioSettings 'Presets'
if (!(Test-Path -LiteralPath $studioPresetTarget)) {
    Copy-Item -LiteralPath $studioPresetSource -Destination $studioPresetTarget -Recurse
}
$studioStart = New-Object System.Diagnostics.ProcessStartInfo
$studioStart.FileName = $studioExe
$studioStart.Arguments = '--in-window-menus "' + $studioSample + '"'
if ($SafeGpu) { $studioStart.Arguments = '--safe-gpu ' + $studioStart.Arguments }
$studioStart.WorkingDirectory = $studioPaintings
$studioStart.UseShellExecute = $false
$studioStart.CreateNoWindow = $true
$studioStart.EnvironmentVariables['PHOTOCRAFT_CONFIG_DIR'] = $studioSettings
$studioStart.EnvironmentVariables['TEMP'] = $studioTemp
$studioStart.EnvironmentVariables['TMP'] = $studioTemp
$studioStart.EnvironmentVariables['PHOTOCRAFT_NO_SYSTEM_FONTS'] = '1'
$studioStart.EnvironmentVariables['RUST_LOG'] = 'warn,photocraft*=info,egui_wgpu=debug,egui_wgpu::winit=warn,egui_wgpu::renderer=warn'
foreach ($studioVariable in @('PHOTOCRAFT_CONTROL_PORT', 'PHOTOCRAFT_INLINE_JOBS', 'PHOTOCRAFT_CPU_CANVAS', 'PHOTOCRAFT_GPU_STARTUP_FAILURE', 'WGPU_BACKEND')) {
    $studioStart.EnvironmentVariables.Remove($studioVariable)
}
$studioProcess = [Diagnostics.Process]::Start($studioStart)
if ($null -eq $studioProcess) { throw 'PhotoCraft could not be started.' }
Write-Output "PhotoCraft started (PID $($studioProcess.Id)); settings and paintings: $studioProfile"
