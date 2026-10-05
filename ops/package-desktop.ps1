$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.IO.Compression

$project = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$artifacts = Join-Path $project 'artifacts'
$windowsDir = Join-Path $artifacts 'windows-x64'
$linuxDir = Join-Path $artifacts 'linux-x64'
$iconPng = Join-Path $project 'assets\mabaeiream-icon.png'
$iconIco = Join-Path $project 'assets\mabaeiream-icon.ico'
$notices = Join-Path $PSScriptRoot 'THIRD_PARTY_NOTICES.md'

$requiredFiles = @(
    (Join-Path $windowsDir 'mabaeiream-desktop-windows-x64-r16.exe'),
    (Join-Path $windowsDir 'runtime\mpv\mpv.exe'),
    (Join-Path $windowsDir 'runtime\mpv\d3dcompiler_43.dll'),
    (Join-Path $windowsDir 'runtime\mpv\yt-dlp.exe'),
    (Join-Path $windowsDir 'runtime\mpv\mpv\fonts.conf'),
    (Join-Path $linuxDir 'mabaeiream-desktop-linux-x64-r16'),
    (Join-Path $linuxDir 'runtime\mpv\mpv-portable.AppImage'),
    (Join-Path $linuxDir 'runtime\mpv\yt-dlp')
)

foreach ($file in $requiredFiles) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
        throw "Cannot package desktop builds; required bundled file is missing: $file"
    }
}

foreach ($file in @($iconPng, $iconIco, $notices)) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
        throw "Cannot package desktop builds; required release metadata is missing: $file"
    }
}

$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$tempPrefix = $tempRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
$stagingRoot = [System.IO.Path]::GetFullPath((Join-Path $tempRoot ("mabaeiream-release-" + [guid]::NewGuid().ToString('N'))))
if (-not $stagingRoot.StartsWith($tempPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to use a release staging directory outside the system temp directory: $stagingRoot"
}

$windowsStage = Join-Path $stagingRoot 'windows-x64'
$linuxStage = Join-Path $stagingRoot 'linux-x64'

function Copy-ReleaseFile {
    param(
        [Parameter(Mandatory)] [string] $Source,
        [Parameter(Mandatory)] [string] $DestinationRoot,
        [Parameter(Mandatory)] [string] $RelativePath
    )

    $destination = Join-Path $DestinationRoot $RelativePath
    New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
    Copy-Item -LiteralPath $Source -Destination $destination -Force
}

function New-PlatformZip {
    param(
        [Parameter(Mandatory)] [string] $SourceDirectory,
        [Parameter(Mandatory)] [string] $Destination,
        [string[]] $ExecutableEntries = @()
    )

    if (Test-Path -LiteralPath $Destination) {
        throw "Refusing to overwrite an existing release ZIP: $Destination"
    }

    $stream = [System.IO.File]::Open($Destination, [System.IO.FileMode]::CreateNew)
    $archive = [System.IO.Compression.ZipArchive]::new($stream, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        Get-ChildItem -LiteralPath $SourceDirectory -Recurse -File | Sort-Object FullName | ForEach-Object {
            $entryName = $_.FullName.Substring($SourceDirectory.Length + 1).Replace('\', '/')
            $entry = $archive.CreateEntry($entryName, [System.IO.Compression.CompressionLevel]::Optimal)
            if ($entryName -in $ExecutableEntries) {
                $entry.ExternalAttributes = [BitConverter]::ToInt32([BitConverter]::GetBytes([Convert]::ToUInt32('81ED0000', 16)), 0)
            }
            $sourceStream = [System.IO.File]::OpenRead($_.FullName)
            $entryStream = $entry.Open()
            try { $sourceStream.CopyTo($entryStream) }
            finally { $entryStream.Dispose(); $sourceStream.Dispose() }
        }
    }
    finally {
        $archive.Dispose()
        $stream.Dispose()
    }
}

try {
    New-Item -ItemType Directory -Path $windowsStage, $linuxStage -Force | Out-Null

    Copy-ReleaseFile (Join-Path $windowsDir 'mabaeiream-desktop-windows-x64-r16.exe') $windowsStage 'mabaeiream-desktop-windows-x64-r16.exe'
    Copy-ReleaseFile (Join-Path $windowsDir 'runtime\mpv\mpv.exe') $windowsStage 'runtime\mpv\mpv.exe'
    Copy-ReleaseFile (Join-Path $windowsDir 'runtime\mpv\d3dcompiler_43.dll') $windowsStage 'runtime\mpv\d3dcompiler_43.dll'
    Copy-ReleaseFile (Join-Path $windowsDir 'runtime\mpv\yt-dlp.exe') $windowsStage 'runtime\mpv\yt-dlp.exe'
    Copy-ReleaseFile (Join-Path $windowsDir 'runtime\mpv\mpv\fonts.conf') $windowsStage 'runtime\mpv\mpv\fonts.conf'

    Copy-ReleaseFile (Join-Path $linuxDir 'mabaeiream-desktop-linux-x64-r16') $linuxStage 'mabaeiream-desktop-linux-x64'
    Copy-ReleaseFile (Join-Path $linuxDir 'runtime\mpv\mpv-portable.AppImage') $linuxStage 'runtime\mpv\mpv-portable.AppImage'
    Copy-ReleaseFile (Join-Path $linuxDir 'runtime\mpv\yt-dlp') $linuxStage 'runtime\mpv\yt-dlp'

    foreach ($stage in @($windowsStage, $linuxStage)) {
        Copy-Item -LiteralPath $iconPng -Destination (Join-Path $stage 'mabaeiream-icon.png') -Force
        Copy-Item -LiteralPath $iconIco -Destination (Join-Path $stage 'mabaeiream-icon.ico') -Force
        Copy-Item -LiteralPath $notices -Destination (Join-Path $stage 'THIRD_PARTY_NOTICES.md') -Force
    }

    $windowsReadme = @'
MaBaeiream for Windows x64

Extract this entire ZIP, then double-click mabaeiream-desktop-windows-x64-r16.exe.
The portable video player and YouTube link runtime are included in runtime\mpv.
No separate media-player installation is required.
'@
    [System.IO.File]::WriteAllText((Join-Path $windowsStage 'README.txt'), $windowsReadme, [System.Text.UTF8Encoding]::new($false))

    $linuxReadme = @'
MaBaeiream for Linux x64

Extract this entire ZIP. From the extracted folder, run:
  sh ./run-mabaeiream.sh

The launcher prepares executable permissions for the included app and portable
video-player files. The Linux app needs glibc 2.39+, ALSA, and X11 or Wayland
libraries; this build was tested on Ubuntu 24.04.
No separate media-player installation or runtime update approval is required.
'@
    [System.IO.File]::WriteAllText((Join-Path $linuxStage 'README.txt'), $linuxReadme, [System.Text.UTF8Encoding]::new($false))

    $linuxLauncher = @'
#!/bin/sh
set -eu
APP_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
chmod +x "$APP_DIR/mabaeiream-desktop-linux-x64" \
  "$APP_DIR/runtime/mpv/mpv.AppImage" \
  "$APP_DIR/runtime/mpv/mpv-portable.AppImage" \
  "$APP_DIR/runtime/mpv/yt-dlp"
exec "$APP_DIR/mabaeiream-desktop-linux-x64" "$@"
'@
    [System.IO.File]::WriteAllText((Join-Path $linuxStage 'run-mabaeiream.sh'), $linuxLauncher, [System.Text.UTF8Encoding]::new($false))

    $linuxPlayerLauncher = @'
#!/bin/sh
set -eu
PLAYER_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
chmod +x "$PLAYER_DIR/mpv-portable.AppImage"
export DISABLE_AUTO_UPDATES=1
exec "$PLAYER_DIR/mpv-portable.AppImage" "$@"
'@
    [System.IO.File]::WriteAllText((Join-Path $linuxStage 'runtime\mpv\mpv.AppImage'), $linuxPlayerLauncher, [System.Text.UTF8Encoding]::new($false))

    New-PlatformZip -SourceDirectory $windowsStage -Destination (Join-Path $artifacts 'mabaeiream-windows-x64-r16.zip')
    New-PlatformZip -SourceDirectory $linuxStage -Destination (Join-Path $artifacts 'mabaeiream-linux-x64-r16.zip') -ExecutableEntries @(
        'mabaeiream-desktop-linux-x64',
        'run-mabaeiream.sh',
        'runtime/mpv/mpv.AppImage',
        'runtime/mpv/mpv-portable.AppImage',
        'runtime/mpv/yt-dlp'
    )
}
finally {
    if (Test-Path -LiteralPath $stagingRoot) {
        Remove-Item -LiteralPath $stagingRoot -Recurse -Force
    }
}

Get-Item (Join-Path $artifacts 'mabaeiream-windows-x64-r16.zip'), (Join-Path $artifacts 'mabaeiream-linux-x64-r16.zip') |
    Select-Object FullName, Length, LastWriteTime

