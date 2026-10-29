# Installs codemycel from the latest GitHub release into %LOCALAPPDATA%\codemycel\bin, then
# runs `codemycel setup` with its answers read from the console.
#   irm https://raw.githubusercontent.com/msaeedsakib/codemycel/main/install.ps1 | iex
# CODEMYCEL_INSTALL_BASE_URL replaces the release download URL.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$base = if ($env:CODEMYCEL_INSTALL_BASE_URL) { $env:CODEMYCEL_INSTALL_BASE_URL } else { 'https://github.com/msaeedsakib/codemycel/releases/latest/download' }
$dir = Join-Path $env:LOCALAPPDATA 'codemycel\bin'
$asset = 'codemycel-x86_64-pc-windows-msvc.zip'

$tmp = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "downloading $asset"
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile (Join-Path $tmp $asset)
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset.sha256" -OutFile (Join-Path $tmp "$asset.sha256")
    $expected = ((Get-Content -Raw (Join-Path $tmp "$asset.sha256")).Trim() -split '\s+')[0].ToLowerInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $asset)).Hash.ToLowerInvariant()
    if ($expected -ne $actual) { throw "codemycel: checksum mismatch for $asset" }

    $unpacked = Join-Path $tmp 'unpacked'
    Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath $unpacked
    $binary = Get-ChildItem -Path $unpacked -Recurse -Filter 'codemycel.exe' | Select-Object -First 1
    if (-not $binary) { throw "codemycel: $asset holds no codemycel.exe" }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Copy-Item -Force -Path $binary.FullName -Destination (Join-Path $dir 'codemycel.exe')
} finally {
    Remove-Item -Recurse -Force -Path $tmp -ErrorAction SilentlyContinue
}
$exe = Join-Path $dir 'codemycel.exe'
Write-Host "installed $exe"

if (-not (($env:Path -split ';') -contains $dir)) {
    Write-Host "warning: $dir is not on your PATH. Add it with:"
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', `"$dir;`" + [Environment]::GetEnvironmentVariable('Path', 'User'), 'User')"
}

$console = $true
try { [IO.File]::Open('CONIN$', 'Open', 'Read').Close() } catch { $console = $false }
if ($console) {
    cmd /c "`"$exe`" setup <CONIN$"
    if ($LASTEXITCODE -ne 0) { Write-Host 'run codemycel setup to configure your agents' }
} else {
    Write-Host 'run codemycel setup to configure your agents'
}
