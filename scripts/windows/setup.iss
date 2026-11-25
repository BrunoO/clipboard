; Inno Setup 6+ Installer Script for CipherClipboard
; Builds a silent-compatible Windows installer for WinGet & GitHub Releases

#define MyAppName "CipherClipboard"
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#define MyAppPublisher "BrunoO"
#define MyAppURL "https://github.com/BrunoO/clipboard"
#define MyAppExeName "CipherClipboard.exe"
#define VCRedistName "vc_redist.x64.exe"

#ifndef SourceExeDir
  #if FileExists("..\..\target\x86_64-pc-windows-msvc\release\" + MyAppExeName)
    #define SourceExeDir "..\..\target\x86_64-pc-windows-msvc\release"
  #else
    #define SourceExeDir "..\..\target\release"
  #endif
#endif

[Setup]
; AppId uniquely identifies this application.
AppId={{034BA591-E20A-4E62-824F-281DEEF982D1}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases

; Default to per-user installation (%LOCALAPPDATA%\Programs) which does not require admin / UAC elevation,
; but allow user to override to administrative (all-users) installation if desired.
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog

; Output configuration
OutputDir=..\..\dist
OutputBaseFilename=CipherClipboard-{#MyAppVersion}-windows-x64-setup
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
LicenseFile=..\..\LICENSE

; Compression & 64-bit targeting
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
; Close any running instance before overwriting files, then relaunch afterwards.
CloseApplications=yes
RestartApplications=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExeDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
; Staged by the release workflow. Skipped for local dev builds, which already
; have the VC++ runtime from Visual Studio Build Tools.
Source: "{#SourceExeDir}\{#VCRedistName}"; DestDir: "{tmp}"; Flags: deleteafterinstall skipifsourcedoesntexist
Source: "..\..\assets\icon.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\icon.png"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\icon.ico"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\icon.ico"; Tasks: desktopicon

[Run]
; The MSVC-linked exe cannot start on a clean Windows install without the
; Universal CRT. vc_redist exits immediately when already installed; skip it
; entirely when vcruntime140.dll is present so silent installs do not trigger
; an unnecessary elevation prompt.
Filename: "{tmp}\{#VCRedistName}"; Parameters: "/install /quiet /norestart"; StatusMsg: "Installing Visual C++ runtime..."; Check: NeedsVCRedist; Flags: waituntilterminated skipifdoesntexist
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Code]
function NeedsVCRedist: Boolean;
begin
  Result := not FileExists(ExpandConstant('{sys}\vcruntime140.dll'));
end;
