; Inno Setup script for the Windows installer.
;
; Build the programs first (cargo build --release), then:
;   iscc /DVersion=0.1.0 packaging\windows\ggez.iss
; Add /DIcon=<path to ggez.ico> to give the installer the program's icon.
; The installer is written to dist\.
;
; It installs for the current user only, so it never asks for
; administrator rights.

#ifndef Version
  #error Pass the version: iscc /DVersion=0.1.0 ggez.iss
#endif

[Setup]
; Identifies ggez to Windows across versions. Never change it.
AppId={{6C1E1B7E-52B4-4B0B-9A0B-6B3B9F0E7A51}
AppName=ggez
AppVersion={#Version}
AppVerName=ggez {#Version}
AppPublisher=The ggez contributors
AppPublisherURL=https://github.com/Thrasherop/ggez
AppSupportURL=https://github.com/Thrasherop/ggez/issues
AppUpdatesURL=https://github.com/Thrasherop/ggez/releases
DefaultDirName={autopf}\ggez
DefaultGroupName=ggez
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=ggez-{#Version}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\ggez-gui.exe
UninstallDisplayName=ggez
; An update replaces the programs, so the helper must not be running.
CloseApplications=yes
RestartApplications=no
#ifdef Icon
SetupIconFile={#Icon}
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "..\..\target\release\ggez-gui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\ggez.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\..\profile.example.toml"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\ggez"; Filename: "{app}\ggez-gui.exe"
Name: "{autodesktop}\ggez"; Filename: "{app}\ggez-gui.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\ggez-gui.exe"; Description: "Open ggez"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Stop the helper and its notification-area icon, then take ggez out of
; the programs that start at login. Settings in %APPDATA%\ggez are kept.
Filename: "{sys}\taskkill.exe"; Parameters: "/im ggez-gui.exe /f"; Flags: runhidden; RunOnceId: "StopHelper"
Filename: "{app}\ggez.exe"; Parameters: "autostart disable"; Flags: runhidden; RunOnceId: "RemoveAutostart"
