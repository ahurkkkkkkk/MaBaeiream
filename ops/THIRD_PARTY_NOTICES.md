# Bundled media runtime notices

MaBaeiream's Windows and Linux desktop ZIPs include separately built media tools so recipients do not need to install a system player.

## mpv

- Windows x64 build: [shinchiro/mpv-winbuild-cmake, release 20260928](https://github.com/shinchiro/mpv-winbuild-cmake/releases/tag/20260928). This is a third-party Windows build; mpv's [installation page](https://mpv.io/installation/) identifies it as such.
- Linux x64 AppImage: [pkgforge-dev/mpv-AppImage, mpv 0.41.0](https://github.com/pkgforge-dev/mpv-AppImage/releases/tag/v0.41.0%402026-09-07_1788787125). This is an unofficial portable AppImage.
- Its optional self-updater is disabled by MaBaeiream when launching playback, so it will not request permission to check for or download updates. Runtime updates ship with MaBaeiream releases.
- mpv source and license information: [mpv project](https://github.com/mpv-player/mpv) and [license](https://github.com/mpv-player/mpv/blob/master/COPYING).

## yt-dlp

- Windows and Linux standalone binaries: [yt-dlp/yt-dlp, release 2026.08.19](https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19).
- yt-dlp is released under the [Unlicense](https://github.com/yt-dlp/yt-dlp/blob/master/LICENSE).

## WebRTC voice runtime

- The static Linux server binary includes [musl libc](https://musl.libc.org/), which is MIT licensed.

- Android includes `io.github.webrtc-sdk:android:150.7871.01`. Its Maven metadata declares the 3-Clause BSD license; source and license are available from [webrtc-sdk/android](https://github.com/webrtc-sdk/android) and [BSD 3-Clause](https://opensource.org/license/bsd-3-clause/).
- Desktop uses [webrtc-rs/webrtc](https://github.com/webrtc-rs/webrtc) and [webrtc-rs/rtc](https://github.com/webrtc-rs/rtc), licensed MIT or Apache-2.0; [cpal](https://github.com/RustAudio/cpal), Apache-2.0; and the [opus-rs bindings](https://github.com/SpaceManiac/opus-rs), MIT or Apache-2.0.
- The desktop audio codec is [Opus](https://gitlab.xiph.org/xiph/opus), distributed under its BSD-style license.

## Pinned asset checksums

These SHA-256 digests are from the corresponding GitHub release asset metadata. They identify the upstream downloads used for this package.

| File | SHA-256 |
|---|---|
| `mpv-x86_64-20260928-git-e470f8986e.7z` | `6491ba670f836553fdd0d965c6e99ed8aa4053c1c79a5e4e30a1e127da64714a` |
| `mpv-v0.41.0-anylinux-x86_64.AppImage` | `bb52fb49c54e83155891bfb97578e7ee40575a306d0dddc96fc603be20db8214` |
| `yt-dlp.exe` | `66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a` |
| `yt-dlp_linux` | `58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a` |

The Linux packaging script only adds executable permission bits required to run the bundled AppImage, yt-dlp binary, and MaBaeiream executable.
