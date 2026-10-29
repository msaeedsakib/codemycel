# Installs codemycel with install.ps1 from release archives in a local directory, then indexes
# the TypeScript fixture with nothing pre-installed and the Rust fixture with only rustup.
#   scripts/clean-machine/windows.ps1 -Artefacts <directory with codemycel-<target>.zip and .sha256> [-Logs <directory>]
# PowerShell 7 dropped file:// from Invoke-WebRequest, so the archives are served on loopback.
param(
    [Parameter(Mandatory = $true)][string]$Artefacts,
    [string]$Logs
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new()

$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Artefacts = (Resolve-Path $Artefacts).Path
$work = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $work | Out-Null
if (-not $Logs) { $Logs = Join-Path $work 'logs' }
New-Item -ItemType Directory -Force -Path $Logs | Out-Null
$Logs = (Resolve-Path $Logs).Path

$cargoBin = Join-Path $(if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }) 'bin'
$env:LOCALAPPDATA = Join-Path $work 'localappdata'
$env:CODEMYCEL_HOME = Join-Path $work 'codemycel-home'
$env:CODEMYCEL_NO_UPDATE_CHECK = '1'
New-Item -ItemType Directory -Path $env:LOCALAPPDATA | Out-Null
$bin = Join-Path $env:LOCALAPPDATA 'codemycel\bin'
$exe = Join-Path $bin 'codemycel.exe'
$basePath = @(
    (Join-Path $env:SystemRoot 'System32'),
    $env:SystemRoot,
    (Join-Path $env:SystemRoot 'System32\Wbem'),
    (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0'),
    $PSHOME
) -join ';'

function Fail([string]$message) { throw "clean-machine: $message" }

function Expect([string]$file, [string]$needle) {
    if (-not (Get-Content -Raw -Encoding utf8 $file).Contains($needle)) { Fail "$file lacks: $needle" }
}

$env:Path = $basePath
foreach ($tool in 'node', 'npm', 'npx', 'tsc', 'tsgo', 'bun', 'deno', 'rust-analyzer') {
    $found = Get-Command $tool -ErrorAction SilentlyContinue
    if ($found) { Fail "$tool is on the base PATH ($($found.Source))" }
}

$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = $listener.LocalEndpoint.Port
$listener.Stop()
$prefix = "http://localhost:$port/"
$server = Start-Job -ArgumentList $Artefacts, $prefix -ScriptBlock {
    param($root, $prefix)
    $http = [Net.HttpListener]::new()
    $http.Prefixes.Add($prefix)
    $http.Start()
    while ($true) {
        $context = $http.GetContext()
        $file = Join-Path $root ([IO.Path]::GetFileName($context.Request.Url.LocalPath))
        if (Test-Path -LiteralPath $file -PathType Leaf) {
            $bytes = [IO.File]::ReadAllBytes($file)
            $context.Response.ContentLength64 = $bytes.Length
            $context.Response.OutputStream.Write($bytes, 0, $bytes.Length)
        } else {
            $context.Response.StatusCode = 404
        }
        $context.Response.Close()
    }
}

try {
    $asset = 'codemycel-x86_64-pc-windows-msvc.zip'
    $ready = $false
    foreach ($attempt in 1..30) {
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "$prefix$asset.sha256" | Out-Null
            $ready = $true
            break
        } catch {
            Start-Sleep -Seconds 1
        }
    }
    if (-not $ready) { Receive-Job $server; Fail "the loopback server at $prefix did not answer" }

    Write-Host '== install'
    $env:CODEMYCEL_INSTALL_BASE_URL = $prefix.TrimEnd('/')
    $installLog = Join-Path $Logs 'install.log'
    $installer = Start-Process -FilePath (Join-Path $PSHOME 'pwsh.exe') -NoNewWindow -PassThru `
        -RedirectStandardOutput $installLog -RedirectStandardError (Join-Path $Logs 'install.err.log') `
        -ArgumentList '-NoProfile', '-NonInteractive', '-Command', "Get-Content -Raw '$repo\install.ps1' | Invoke-Expression"
    if (-not $installer.WaitForExit(180000)) {
        $installer.Kill()
        Get-Content $installLog
        Fail 'install.ps1 did not finish within three minutes; codemycel setup is probably waiting on the console'
    }
    Remove-Item Env:\CODEMYCEL_INSTALL_BASE_URL
    Get-Content $installLog
    Get-Content (Join-Path $Logs 'install.err.log')
    if ($installer.ExitCode -ne 0) { Fail "install.ps1 exited with $($installer.ExitCode)" }
    Expect $installLog "installed $exe"
    Expect $installLog "$bin is not on your PATH"
    Expect $installLog 'run codemycel setup to configure your agents'
    & $exe --version
    if ($LASTEXITCODE -ne 0) { Fail 'codemycel --version failed' }
} finally {
    Stop-Job $server
    Remove-Job -Force $server
}

# Indexes a copy of fixtures\<fixture> with PATH set to <path>, without the committed SCIP
# index, so the language server does the verification.
function Index([string]$name, [string]$fixture, [string]$path) {
    $copy = Join-Path $work $name
    Copy-Item -Recurse -Path (Join-Path $repo "fixtures\$fixture") -Destination $copy
    Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $copy 'index.scip')
    $env:Path = $path
    Write-Host "== init $name"
    $log = Join-Path $Logs "$name-init.log"
    & $exe init --repo $copy *> $log
    $code = $LASTEXITCODE
    Get-Content $log
    if ($code -ne 0) { Fail "init on $fixture exited with $code" }
    $status = Join-Path $Logs "$name-status.json"
    & $exe status --json --repo $copy > $status
    if ($LASTEXITCODE -ne 0 -or (Get-Content -Raw $status).Contains('degraded')) {
        Get-Content $status
        Fail "$fixture has a degraded root"
    }
}

function Symbol([string]$name, [string]$target) {
    $out = Join-Path $Logs "$name-symbol.txt"
    & $exe symbol $target --repo (Join-Path $work $name) > $out
    if ($LASTEXITCODE -ne 0) { Fail "symbol $target exited with $LASTEXITCODE" }
    Get-Content -Encoding utf8 $out
}

Index 'typescript' 'typescript/monorepo-basic' "$bin;$basePath"
Expect (Join-Path $Logs 'typescript-init.log') '(download), verified by lsp'
Symbol 'typescript' 'packages/core/src/math.ts::double'
Expect (Join-Path $Logs 'typescript-symbol.txt') "$([char]0x2192) calls  packages/core/src/math.ts::scale (function)"

if (-not (Test-Path (Join-Path $cargoBin 'rustup.exe'))) { Fail "rustup is not in $cargoBin" }
Index 'rust' 'rust/workspace-basic' "$bin;$cargoBin;$basePath"
Expect (Join-Path $Logs 'rust-init.log') 'root . (rust): verified by lsp'
Symbol 'rust' 'crates/core/src/lib.rs::total_area'
Expect (Join-Path $Logs 'rust-symbol.txt') "$([char]0x2190) calls  crates/cli/src/main.rs::main (function)"
Expect (Join-Path $Logs 'rust-symbol.txt') "$([char]0x2192) calls  crates/core/src/geometry.rs::Area::area (method)"

Write-Host 'clean-machine: ok'
