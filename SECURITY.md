# Security

Report vulnerabilities privately through
[GitHub Security Advisories](https://github.com/skhortiuk/caffeinate-windows/security/advisories/new).
Please do not open a public issue for security reports.

The installer and `caffeinate --update` both verify the SHA256 published with
each release before writing the binary. `--update` talks to GitHub through
WinHTTP and hashes with BCrypt; it never spawns a shell or runs a script.

The binary is not code-signed, so Windows Defender SmartScreen may warn on a
browser download.
