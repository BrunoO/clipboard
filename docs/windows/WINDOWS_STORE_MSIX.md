# Microsoft Store MSIX Packaging & Publishing Guide

This document describes how CipherClipboard is packaged as an MSIX application for distribution via the **Microsoft Store** and **WinGet**, providing free, trusted Authenticode signing from Microsoft.

---

## 1. Overview & Architecture

* **Package Format:** MSIX (Desktop Bridge / Centennial)
* **Execution Level:** Full Trust (`runFullTrust` capability, `packagedClassicApp` / `Windows.FullTrustApplication`)
* **Dependencies:** Bundles `libcrypto-3-x64.dll` and VC++ runtime DLLs, plus declares `Microsoft.VCLibs.140.00.UWPDesktop`.
* **Signing Model:**
  * **In CI:** The package is packed using the Windows SDK `MakeAppx.exe` and signed with a temporary self-signed certificate matching the configured publisher. This validates structural integrity and allows local testing.
  * **In Microsoft Store:** When uploaded to Partner Center, Microsoft validates the package and re-signs it with Microsoft's official trusted root certificate. End users receive an Authenticode-signed package with no SmartScreen warnings.

---

## 2. GitHub Repository Configuration

To customize the MSIX manifest with your Partner Center registration details, configure these optional repository variables in GitHub (**Settings** $\rightarrow$ **Secrets and variables** $\rightarrow$ **Actions** $\rightarrow$ **Variables**):

| Variable Name | Description | Example Value | Default (if unset) |
| :--- | :--- | :--- | :--- |
| `MSSTORE_PACKAGE_NAME` | **Package/Identity/Name** from Partner Center | `12345BrunoO.CipherClipboard` | `BrunoO.CipherClipboard` |
| `MSSTORE_PUBLISHER` | **Package/Identity/Publisher** from Partner Center | `CN=XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX` | `CN=BrunoO-CipherClipboard` |
| `MSSTORE_PUBLISHER_DISPLAY_NAME` | **Publisher display name** from Partner Center | `BrunoO` | `BrunoO` |

> [!NOTE]
> If these variables are not set in GitHub repository settings, the workflow falls back to the sensible defaults listed above, allowing the CI workflow to build and test-sign cleanly.

---

## 3. Partner Center Product Identity

To find your exact identity strings:

1. Sign in to [Microsoft Partner Center](https://partner.microsoft.com/dashboard/apps-and-games/overview).
2. Select **CipherClipboard** from your apps list.
3. In the sidebar, navigate to **Product management** $\rightarrow$ **Product Identity**.
4. Copy the values for:
   * **Package/Identity/Name**
   * **Package/Identity/Publisher**
   * **Publisher display name**
   * **Store ID** *(e.g. `9NXXXXXXXXXX`)*

---

## 4. Local Packaging

To manually package the MSIX on a Windows development machine:

```powershell
# Build release executable
cargo build --release

# Run packaging script
.\scripts\windows\package-msix.ps1 -Version "0.1.0" `
    -PackageName "BrunoO.CipherClipboard" `
    -Publisher "CN=XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX" `
    -PublisherDisplayName "BrunoO"
```

The resulting package will be in `dist\CipherClipboard-0.1.0-windows-x64.msix`.

---

## 5. Store Submission Guide

When submitting the generated `.msix` in Partner Center:

1. **Packages:**
   * Upload `CipherClipboard-<version>-windows-x64.msix`.
2. **Properties & Categories:**
   * Primary Category: **Productivity**
   * Secondary Category: **Utilities & tools**
3. **Restricted Capability (`runFullTrust`):**
   * When prompted to explain the use of `runFullTrust`, provide:
     > *"CipherClipboard is a desktop Win32 productivity tool that requires full trust to monitor the system clipboard, register global keyboard shortcuts (summon hotkey), and simulate paste key events into the active foreground window."*
4. **Age Ratings (IARC):**
   * Complete the short questionnaire (desktop utility, no user-generated online content, offline encrypted storage $\rightarrow$ rating 3+ / Everyone).
5. **Store Listings:**
   * Provide a short description, feature list, search keywords, and at least one desktop screenshot (16:9, e.g. 1920×1080).
   * Privacy Policy URL: link to `https://github.com/BrunoO/clipboard/blob/main/LICENSE` or a hosted privacy policy.

---

## 6. WinGet Integration

Once published to the Microsoft Store, users can install CipherClipboard via WinGet directly:

```powershell
winget install --id <YourStoreId> --source msstore
```

You can also submit a package manifest to the community repository (`microsoft/winget-pkgs`) declaring:
```yaml
PackageIdentifier: BrunoO.CipherClipboard
PackageVersion: 0.1.0
InstallerType: msstore
Installers:
  - Architecture: x64
    PackageIdentifier: <YourStoreId>
```
This enables `winget install CipherClipboard` without hosting separate binaries or managing third-party code signing certificates.
