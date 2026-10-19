# Security policy

Riptide downloads files from servers it does not control and stores credentials people give it.
That makes security a feature, not a chore. This page explains what the project protects against
and how to report a problem.

## Reporting a vulnerability

Please do **not** open a public issue for a security problem.

Use GitHub's private vulnerability reporting for this repository:
<https://github.com/LayanJarrar1/Riptide/security/advisories/new>. Describe the issue, how to
reproduce it, and what you think the impact is. You will get an acknowledgement, and a fix or a
plan, as quickly as a small project can manage. Credit is given in the release notes unless you
prefer otherwise.

## Supported versions

Pre-releases are published on [GitHub Releases](https://github.com/LayanJarrar1/Riptide/releases).
The latest pre-release is the supported version; please reproduce problems against it, or against
`main` if the fix is already there.

## What Riptide guards against

**Hostile servers**

- File names from `Content-Disposition` or the URL are sanitised: directory parts and `..` are
  stripped, control characters removed, Windows reserved names (`CON`, `PRN`, `AUX`, `NUL`,
  `COM1`–`COM9`, `LPT1`–`LPT9`) renamed, trailing dots and spaces trimmed, the name is
  NFC-normalised and capped at 200 bytes. The engine refuses to write outside the chosen folder.
- Redirect chains are capped at 20 hops. Cookies and `Authorization` headers are dropped when a
  redirect leaves the origin they were captured for.
- `Content-Length` is checked against the bytes received; a mismatch fails the download rather
  than producing a truncated or padded file.
- Resume uses `If-Range` with the server's validator. If the file changed, the download restarts
  from zero; bytes from two versions are never combined.
- TLS verification is on. Ignoring certificate errors is a per-download switch that shows a
  warning and is never the default.
- Optional checksum verification (MD5, SHA-1, SHA-256) after completion.

**Credentials and cookies**

- Stored in a separate `secrets` table, deleted when the download completes or is removed.
- Sent only to the origin they were captured for.
- Never written to logs; a redaction layer strips them and URLs are logged without query strings.

**The local system**

- Per-user install, data in `%LOCALAPPDATA%\Riptide`, registry use under `HKCU` only, no
  elevation anywhere.
- The local IPC endpoint (named pipe / Unix socket) is created with an ACL that admits only the
  current user.
- Nothing received over IPC or from a browser is executed, shelled, or used as a file-system
  path without going through the same sanitiser as everything else.

**Privacy**

- No telemetry, no accounts, no ads. Network traffic is the user's downloads, the local browser
  bridge, and an optional update check that can be turned off.

## Scope notes

The engine is used against untrusted input by design, so reports about malformed responses,
unusual headers, pathological file names, path traversal attempts and resume edge cases are
exactly the kind the project wants. Reports about third-party dependencies are welcome too; please
mention the crate or package and the version.
