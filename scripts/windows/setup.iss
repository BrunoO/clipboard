; Inno Setup 6+ Installer Script for CipherClipboard
; Builds a silent-compatible Windows installer for WinGet & GitHub Releases

#define MyAppName "CipherClipboard"
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#define MyAppPublisher "BrunoO"
#define MyAppURL "https://github.com/BrunoO/clipboard"
#define MyAppExeName "CipherClipboard.exe"

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
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExeDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\icon.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\icon.png"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\icon.ico"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\icon.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
