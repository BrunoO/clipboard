# PowerShell Script to Package CipherClipboard as MSIX for Microsoft Store & GitHub Releases
#
# Locates the Windows 10/11 SDK MakeAppx.exe & SignTool.exe, stages required files,
# generates AppxManifest.xml, packs the MSIX, and signs it with a matching certificate.

param(
    [string]$Version = "",
    [string]$PackageName = "",
    [string]$Publisher = "",
    [string]$PublisherDisplayName = "",
    [string]$OpenSslDllName = ""
)

$ErrorActionPreference = "Stop"

# 1. Resolve parameters and defaults
if ([string]::IsNullOrWhiteSpace($Version)) {
    if (-not [string]::IsNullOrWhiteSpace($env:VERSION)) {
        $Version = $env:VERSION
    } else {
        $Version = "0.1.0"
    }
}
$rawVersion = $Version.TrimStart('v')

# MSIX requires a 4-part numeric version: Major.Minor.Build.Revision (e.g. 0.1.0.0)
$versionParts = @($rawVersion.Split('.'))
while ($versionParts.Count -lt 4) {
    $versionParts += "0"
}
$msixVersion = ($versionParts[0..3] -join '.')

if ([string]::IsNullOrWhiteSpace($PackageName)) {
    if (-not [string]::IsNullOrWhiteSpace($env:MSSTORE_PACKAGE_NAME)) {
        $PackageName = $env:MSSTORE_PACKAGE_NAME
    } else {
        $PackageName = "BrunoO.CipherClipboard"
    }
}

if ([string]::IsNullOrWhiteSpace($Publisher)) {
    if (-not [string]::IsNullOrWhiteSpace($env:MSSTORE_PUBLISHER)) {
        $Publisher = $env:MSSTORE_PUBLISHER
    } else {
        $Publisher = "CN=BrunoO-CipherClipboard"
    }
}

if ([string]::IsNullOrWhiteSpace($PublisherDisplayName)) {
    if (-not [string]::IsNullOrWhiteSpace($env:MSSTORE_PUBLISHER_DISPLAY_NAME)) {
        $PublisherDisplayName = $env:MSSTORE_PUBLISHER_DISPLAY_NAME
    } else {
        $PublisherDisplayName = "Bruno O."
    }
}
if ($PublisherDisplayName -eq "Bruno O") {
    $PublisherDisplayName = "Bruno O."
}

if ([string]::IsNullOrWhiteSpace($OpenSslDllName)) {
    if (-not [string]::IsNullOrWhiteSpace($env:OPENSSL_RUNTIME_DLL)) {
        $OpenSslDllName = $env:OPENSSL_RUNTIME_DLL
    } else {
        $OpenSslDllName = "libcrypto-3-x64.dll"
    }
}

Write-Host "================ MSIX Packaging Configuration ================"
Write-Host "Package Version:        $msixVersion (from $rawVersion)"
Write-Host "Package Name:           $PackageName"
Write-Host "Publisher:              $Publisher"
Write-Host "Publisher Display Name: $PublisherDisplayName"
Write-Host "OpenSSL DLL:            $OpenSslDllName"
Write-Host "==============================================================="

# 2. Prepare staging directory
$stageDir = "target\msix_stage"
if (Test-Path $stageDir) {
    Remove-Item -Recurse -Force $stageDir
}
New-Item -ItemType Directory -Force -Path "$stageDir\Assets" | Out-Null

# 3. Stage executable and OpenSSL DLL
$exeSource = "target\release\CipherClipboard.exe"
if (-not (Test-Path $exeSource)) {
    throw "Executable not found at $exeSource. Run 'cargo build --release' first."
}
Copy-Item $exeSource -Destination $stageDir

$dllSource = "target\release\$OpenSslDllName"
if (Test-Path $dllSource) {
    Copy-Item $dllSource -Destination $stageDir
    Write-Host "Staged OpenSSL runtime DLL: $OpenSslDllName"
} else {
    Write-Warning "OpenSSL runtime DLL not found at $dllSource."
}

# 4. Stage VC++ runtime DLLs for self-contained reliability
$sys32 = [System.Environment]::SystemDirectory
foreach ($vcDll in @("vcruntime140.dll", "msvcp140.dll", "vcruntime140_1.dll")) {
    $srcVc = Join-Path $sys32 $vcDll
    if (Test-Path $srcVc) {
        Copy-Item $srcVc -Destination $stageDir -Force
        Write-Host "Staged VC++ runtime DLL: $vcDll"
    }
}

# 5. Stage visual assets
$assetsSource = "scripts\windows\msix\Assets"
if (-not (Test-Path $assetsSource)) {
    throw "MSIX assets directory not found at $assetsSource"
}
Copy-Item "$assetsSource\*" -Destination "$stageDir\Assets"

# 6. Generate AppxManifest.xml
$manifestTemplatePath = "scripts\windows\msix\AppxManifest.xml"
if (-not (Test-Path $manifestTemplatePath)) {
    throw "AppxManifest template not found at $manifestTemplatePath"
}
$manifestContent = Get-Content $manifestTemplatePath -Raw
$processedManifest = $manifestContent `
    -replace '__PACKAGE_IDENTITY_NAME__', $PackageName `
    -replace '__PACKAGE_IDENTITY_PUBLISHER__', $Publisher `
    -replace '__PUBLISHER_DISPLAY_NAME__', $PublisherDisplayName `
    -replace '__PACKAGE_VERSION__', $msixVersion

$targetManifest = "$stageDir\AppxManifest.xml"
$processedManifest | Set-Content $targetManifest -Encoding UTF8
Write-Host "Generated AppxManifest.xml at $targetManifest"

# 7. Locate Windows SDK MakeAppx.exe and SignTool.exe
$kitsRoot = "C:\Program Files (x86)\Windows Kits\10\bin"
if (-not (Test-Path $kitsRoot)) {
    throw "Windows Kits 10 bin folder not found at $kitsRoot"
}

$sdkVersions = @(Get-ChildItem -Path $kitsRoot -Directory | Where-Object { $_.Name -like "10.*" } | Sort-Object Name -Descending)
if ($sdkVersions.Count -eq 0) {
    throw "No Windows 10/11 SDK versions found under $kitsRoot"
}

$selectedSdk = $sdkVersions[0].FullName
$makeAppx = Join-Path $selectedSdk "x64\MakeAppx.exe"
$makePri  = Join-Path $selectedSdk "x64\MakePri.exe"
$signTool = Join-Path $selectedSdk "x64\SignTool.exe"

if (-not (Test-Path $makeAppx)) {
    throw "MakeAppx.exe not found at $makeAppx"
}
if (-not (Test-Path $makePri)) {
    throw "MakePri.exe not found at $makePri"
}
if (-not (Test-Path $signTool)) {
    throw "SignTool.exe not found at $signTool"
}
Write-Host "Using Windows SDK tools from $selectedSdk"

# 8. Generate Package Resource Index (resources.pri)
Write-Host "Creating PRI configuration..."
$priConfig = "$stageDir\priconfig.xml"
& $makePri createconfig /cf $priConfig /dq "en-US" /pv 10.0.0 /o
if ($LASTEXITCODE -ne 0) {
    throw "MakePri.exe createconfig failed with exit code $LASTEXITCODE"
}

# Remove autoResourcePackage elements if present so MakePri creates a single standalone resources.pri
[xml]$priXml = Get-Content $priConfig
$packagingNode = $priXml.SelectSingleNode("//packaging")
if ($packagingNode) {
    $packagingNode.ParentNode.RemoveChild($packagingNode) | Out-Null
    $priXml.Save($priConfig)
}

Write-Host "Compiling Package Resource Index (resources.pri)..."
& $makePri new /pr $stageDir /cf $priConfig /mn $targetManifest /of "$stageDir\resources.pri" /o
if ($LASTEXITCODE -ne 0) {
    throw "MakePri.exe new failed with exit code $LASTEXITCODE"
}
Remove-Item $priConfig -Force
Write-Host "Generated resources.pri at $stageDir\resources.pri"

# 9. Create MSIX package
New-Item -ItemType Directory -Force -Path "dist" | Out-Null
$outputMsix = "dist\CipherClipboard-$rawVersion-windows-x64.msix"
if (Test-Path $outputMsix) {
    Remove-Item -Force $outputMsix
}

Write-Host "Building MSIX package: $outputMsix"
& $makeAppx pack /l /d $stageDir /p $outputMsix /o
if ($LASTEXITCODE -ne 0) {
    throw "MakeAppx.exe failed with exit code $LASTEXITCODE"
}

# 10. Test-sign MSIX
# Note: For Microsoft Store submission, Microsoft strips and re-signs the package
# with Microsoft's official store key upon publication. Signing in CI ensures structural
# integrity, validates publisher matching, and allows local sideload testing.
Write-Host "Creating temporary signing certificate matching publisher: $Publisher"
$cert = New-SelfSignedCertificate `
    -Type Custom `
    -Subject $Publisher `
    -KeyUsage DigitalSignature `
    -FriendlyName "CipherClipboard CI Signing" `
    -CertStoreLocation "Cert:\CurrentUser\My" `
    -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3")

Write-Host "Signing MSIX with certificate thumbprint: $($cert.Thumbprint)"
& $signTool sign /fd SHA256 /sha1 $cert.Thumbprint $outputMsix
if ($LASTEXITCODE -ne 0) {
    throw "SignTool.exe failed with exit code $LASTEXITCODE"
}

Write-Host "MSIX packaged and signed successfully: $outputMsix"
