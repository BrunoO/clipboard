# Single source of truth for Windows OpenSSL setup in CI and release builds.
# The old workflows ran `choco install openssl` but then pointed OPENSSL_DIR at
# C:\Program Files\OpenSSL, which is only a GitHub-hosted runner preinstall:
# a self-hosted runner silently used whatever happened to be there.
# They also flattened every nested .lib into one directory, so the MD/MT CRT
# variants overwrote each other (last write wins).

$ErrorActionPreference = "Stop"

# A self-hosted runner may already provide OpenSSL (e.g. via vcpkg). Otherwise
# install it via Chocolatey and locate the installed tree (Chocolatey openssl
# v4+ deploys to C:\Program Files\OpenSSL while older releases used OpenSSL-Win64).
if ([string]::IsNullOrWhiteSpace($env:OPENSSL_DIR)) {
    choco install openssl -y --no-progress
    if ($LASTEXITCODE -ne 0) {
        # Chocolatey may report a nonzero code when the package is already
        # installed. The header/lib validation below is the real gate.
        Write-Host "choco install returned $LASTEXITCODE; verifying the install tree directly"
    }
    $candidates = @(
        (Join-Path $env:ProgramFiles "OpenSSL"),
        (Join-Path $env:ProgramFiles "OpenSSL-Win64")
    )
    $opensslDir = $null
    foreach ($candidate in $candidates) {
        if (Test-Path (Join-Path $candidate "include\openssl\ssl.h") -PathType Leaf) {
            $opensslDir = $candidate
            break
        }
    }
    if (-not $opensslDir) {
        throw "OpenSSL installation not found (checked: $($candidates -join ', '))"
    }
} else {
    $opensslDir = $env:OPENSSL_DIR
    Write-Host "Using preconfigured OPENSSL_DIR: $opensslDir"
}

if (-not (Test-Path $opensslDir -PathType Container)) {
    throw "OpenSSL directory not found: $opensslDir"
}
$opensslDir = (Resolve-Path $opensslDir).Path

$includeDir = Join-Path $opensslDir "include"
if (-not (Test-Path (Join-Path $includeDir "openssl\ssl.h"))) {
    throw "OpenSSL headers not found under $includeDir"
}

# Rust's MSVC target links the dynamic CRT by default, so the MD import library
# is the one that matches. Prefer it explicitly; never flatten all nested libs.
$libDir = $null
foreach ($candidate in @(
    (Join-Path $opensslDir "lib\VC\x64\MD"),
    (Join-Path $opensslDir "lib"),
    (Join-Path $opensslDir "lib64")
)) {
    if (Test-Path (Join-Path $candidate "libcrypto.lib") -PathType Leaf) {
        $libDir = $candidate
        break
    }
}
if (-not $libDir) {
    throw "libcrypto.lib not found under $opensslDir (looked in lib\VC\x64\MD, lib, lib64)"
}

# Resolve the exact runtime DLL name (e.g. libcrypto-4-x64.dll for OpenSSL 4.x
# or libcrypto-3-x64.dll for OpenSSL 3.x) instead of hardcoding a major version.
$binDir = Join-Path $opensslDir "bin"
$dllCandidates = @()
if (Test-Path $binDir -PathType Container) {
    $dllCandidates = @(Get-ChildItem -Path $binDir -Filter "libcrypto-*-x64.dll" -File)
}
if ($dllCandidates.Count -eq 0) {
    $dllCandidates = @(Get-ChildItem -Path $opensslDir -Filter "libcrypto-*-x64.dll" -File -Recurse)
}
$dllNames = @($dllCandidates | Select-Object -ExpandProperty Name -Unique)
if ($dllNames.Count -eq 0) {
    throw "No libcrypto-*-x64.dll runtime DLL found under $opensslDir"
}
if ($dllNames.Count -gt 1) {
    throw "Multiple libcrypto runtime DLLs found under $opensslDir ($($dllNames -join ', ')); refusing to guess"
}
$runtimeDll = $dllNames[0]

# Ensure the OpenSSL runtime DLL directory is on PATH for test binaries.
if (Test-Path $binDir -PathType Container) {
    $binDir >> $env:GITHUB_PATH
}

# These are consumed by libsqlite3-sys and by the DLL staging step in release.yml.
"OPENSSL_DIR=$opensslDir" >> $env:GITHUB_ENV
"OPENSSL_LIB_DIR=$libDir" >> $env:GITHUB_ENV
"OPENSSL_INCLUDE_DIR=$includeDir" >> $env:GITHUB_ENV
"OPENSSL_RUNTIME_DLL=$runtimeDll" >> $env:GITHUB_ENV

Write-Host "OPENSSL_DIR=$opensslDir"
Write-Host "OPENSSL_LIB_DIR=$libDir"
Write-Host "OPENSSL_INCLUDE_DIR=$includeDir"
Write-Host "OPENSSL_RUNTIME_DLL=$runtimeDll"
