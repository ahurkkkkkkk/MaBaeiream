# MaBaeiream privacy policy draft

**This file must be completed and hosted by the service owner before a public Play release. Replace the bracketed contact field and align this text with the actual server logs and deployment.**

MaBaeiream is a private media client for a server controlled by its owner. Account, library, download, and watch-room requests use the HTTPS server address entered by the user. Playing an external source also contacts that source's host or CDN.

## Information handled

- Account username and password are sent to the configured MaBaeiream server for sign-in. The server stores an Argon2 password hash, not the password itself. The Android app does not save the password.
- The server issues an opaque session token. The Android client keeps it in memory for the current run and discards it on sign-out or process termination. The server stores only a hash of the token with its expiry.
- Library paths, media names, and media bytes are read from the owner's configured server library when requested.
- A direct HTTP(S) download link entered by the user is sent to their MaBaeiream server, which fetches the link and saves the result in that server's library. The downloader restricts targets to public addresses and checks redirects.
- On Android, supported YouTube and Vimeo links are sent to the configured MaBaeiream server for `yt-dlp` resolution. The server contacts the provider, returns temporary video/audio stream URLs and limited playback headers, and Android streams those tracks from the provider's CDN. Desktop resolves supported provider links locally with its bundled `yt-dlp` runtime. The provider and CDN process these requests under their own privacy terms.
- The server operator's reverse proxy or hosting provider may process connection metadata such as IP address and timestamps according to the operator's configuration.

MaBaeiream does not include analytics or advertising SDKs in this source version.

## Storage and sharing

Media remains on the server. The Android app does not request access to shared device storage. The server is intended for two accounts and is not a public hosting service. The owner controls server backups, account provisioning, network access, and deletion.

Sign out revokes the current server session. To delete an account, the server owner must remove it from the server's account database and remove any backups containing that account data.

## Security

The clients require HTTPS and use the platform's certificate validation. Do not enter credentials into an untrusted server address. Do not publish server passwords, signing keys, media URLs containing secrets, or database files.

## Contact

Privacy questions: **[add a contact email or support URL controlled by the service owner]**.
