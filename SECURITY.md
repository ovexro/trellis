# Security Policy

## Reporting a Vulnerability

If you've found a security issue in Trellis, please report it privately rather than opening a public GitHub issue.

**Email:** nubiraorg@gmail.com

Please include:

- A description of the vulnerability and the impact you believe it has
- Steps to reproduce (a minimal proof-of-concept is ideal)
- The affected component (desktop app, Arduino library, embedded `:9090` web UI, MQTT bridge, etc.) and version
- Whether you'd like to be credited in the fix announcement (and how)

You should expect a first response within 7 days. We aim to ship a fix or a documented mitigation within 30 days for confirmed issues, sooner for actively exploitable ones.

## Scope

In scope:

- The desktop application (Tauri / Rust + React) and its REST/WebSocket APIs
- The embedded `:9090` web dashboard served by the desktop app
- The Arduino library (`src/`) including the embedded HTTP / WebSocket / OTA servers
- Authentication, authorization, token handling, secret storage
- The MQTT bridge, Sinric Pro integration, remote-access tunnels

Out of scope (please do not report these as security issues):

- Issues that require physical access to an unlocked machine
- Self-inflicted misconfiguration (e.g. exposing your unauthenticated `:9090` to the public internet without a tunnel)
- Vulnerabilities in upstream dependencies that have already been disclosed and have a CVE; please report those upstream
- Social-engineering attacks against project maintainers

## Supported Versions

Only the most recent minor release receives security fixes. Older versions may receive a fix on a best-effort basis if the issue is severe.

## Hall of Fame

Researchers who report valid issues will be credited (with permission) in the release notes for the fix and in this file.
