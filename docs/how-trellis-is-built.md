# How Trellis Is Built — A Short Guide

## The Code: Rust + React

Trellis is a desktop app built with **Tauri 2**. It has two halves:

```
app/src-tauri/src/   <-- Rust (backend)
app/src/             <-- React + TypeScript (frontend UI)
```

**Rust** (the backend) handles everything "behind the scenes":
- Discovering devices on your network (mDNS)
- Maintaining WebSocket connections to each device
- Storing data in SQLite (metrics, alerts, schedules)
- Serving the REST API on port 9090
- OTA firmware updates
- System tray

**React** (the frontend) is the visual interface you see:
- Device cards, charts, controls, settings pages
- Rendered inside a WebView (like a lightweight browser window)

### How They Talk to Each Other

React can't access your network or database directly. It asks Rust:

```
React:  invoke("get_devices")          -->  Rust: queries SQLite, returns JSON
React:  invoke("send_command", {...})   -->  Rust: opens WebSocket to ESP32
```

Rust can also push data to React without being asked:

```
Rust:   app_handle.emit("device-discovered", device_data)
React:  listen("device-discovered", (event) => { update UI })
```

This is how device discovery works in real-time — Rust finds a device via mDNS, emits an event, React updates the dashboard instantly.

## The Workflow: Edit, Commit, Push, Build, Release

### 1. Edit Code

You change files in `~/trellis/`. Nothing happens yet — the changes only exist on disk.

### 2. Commit (local save)

```bash
git add <files>
git commit -m "description of what changed"
```

A commit is a **snapshot** — it records exactly what every file looks like at that moment. Commits are LOCAL. They're on your machine only. You can make 50 commits and nobody else sees them.

### 3. Push (upload to GitHub)

```bash
git push
```

Push sends your commits to GitHub. Now the code is:
- Backed up (not just on your PC)
- Visible to anyone who visits the repo
- Available for CI to build

**But**: pushing does NOT create a release or a downloadable package. It just updates the source code on GitHub.

### 4. Build the .deb (compile into an installer)

```bash
cd app && npm run tauri build
```

This takes ~30 seconds and:
1. Compiles Rust into a native Linux binary
2. Bundles the React frontend into that binary
3. Packages everything into `.deb` (Ubuntu/Mint), `.rpm` (Fedora), and `.AppImage`

The result is at: `app/src-tauri/target/release/bundle/deb/Trellis_X.Y.Z_amd64.deb`

### 5. Install locally

```bash
sudo dpkg -i app/src-tauri/target/release/bundle/deb/Trellis_X.Y.Z_amd64.deb
```

Now "Trellis" in your app menu runs the new version.

### 6. Release (make it downloadable for others)

```bash
git tag v0.1.5
git push origin v0.1.5
```

Pushing a **tag** (not just code) triggers GitHub Actions CI, which:
1. Spins up a fresh Linux VM on GitHub's servers
2. Clones your repo and runs `npm run tauri build`
3. Uploads the `.deb`, `.rpm`, and `.AppImage` to **GitHub Releases**

Now anyone can download Trellis from the Releases page or use the install script.

## Three Separate Things

| What | Updated by | Contains |
|------|-----------|----------|
| **Your local install** | `sudo dpkg -i ...` | The binary you actually run |
| **GitHub repo** | `git push` | Source code (what developers see) |
| **GitHub Releases** | `git push origin vX.Y.Z` (tag) | Downloadable packages (what users get) |

These are independent. Pushing code doesn't update your local install. Your local install doesn't update GitHub Releases. You must do each one separately.

## The Arduino Library

The `library/` directory is a separate Arduino library that runs on ESP32 and Pico W. It doesn't get compiled into the desktop app — it's what users flash onto their microcontrollers. It has its own version number that should match the app.
