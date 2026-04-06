#!/usr/bin/env bash
#
# Trellis Installer
# The easiest way to deploy and control ESP32 and Pico W devices.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/ovexro/trellis/main/install.sh | bash
#
# MIT License — https://github.com/ovexro/trellis

set -euo pipefail

# ─── Colors & formatting ─────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

ok()   { echo -e " ${GREEN}[✓]${NC} $*"; }
info() { echo -e " ${BLUE}[→]${NC} $*"; }
warn() { echo -e " ${YELLOW}[!]${NC} $*"; }
fail() { echo -e " ${RED}[✗]${NC} $*"; exit 1; }
ask()  { echo -en " ${CYAN}[?]${NC} $* "; }

# ─── Header ──────────────────────────────────────────────────────────────────

echo ""
echo -e "${GREEN}${BOLD}"
echo "  ╔══════════════════════════════════════════╗"
echo "  ║         Trellis Installer                ║"
echo "  ║   ESP32 & Pico W Device Control Center   ║"
echo "  ╚══════════════════════════════════════════╝"
echo -e "${NC}"

# ─── Detect system ───────────────────────────────────────────────────────────

ARCH=$(uname -m)
case "$ARCH" in
    x86_64)  ARCH_LABEL="x86_64"; DEB_ARCH="amd64" ;;
    aarch64) ARCH_LABEL="aarch64"; DEB_ARCH="arm64" ;;
    armv7l)  ARCH_LABEL="armv7l"; DEB_ARCH="armhf" ;;
    *)       fail "Unsupported architecture: $ARCH" ;;
esac

# Detect distro
DISTRO="unknown"
PKG_MANAGER="unknown"
if [ -f /etc/os-release ]; then
    . /etc/os-release
    DISTRO="$NAME"
    case "$ID" in
        ubuntu|linuxmint|debian|pop|elementary|zorin|kali)
            PKG_MANAGER="apt"
            ;;
        fedora|rhel|centos|rocky|alma)
            PKG_MANAGER="dnf"
            ;;
        arch|manjaro|endeavouros)
            PKG_MANAGER="pacman"
            ;;
        opensuse*|sles)
            PKG_MANAGER="zypper"
            ;;
        *)
            PKG_MANAGER="generic"
            ;;
    esac
fi

ok "Detected: ${BOLD}$DISTRO${NC} ($ARCH_LABEL)"

# ─── Check prerequisites ────────────────────────────────────────────────────

if ! command -v curl &>/dev/null && ! command -v wget &>/dev/null; then
    fail "curl or wget is required. Install with: sudo apt install curl"
fi

DOWNLOADER="curl -fsSL"
if ! command -v curl &>/dev/null; then
    DOWNLOADER="wget -qO-"
fi

# ─── Determine latest release ───────────────────────────────────────────────

REPO="ovexro/trellis"
info "Checking latest release..."

RELEASE_URL="https://api.github.com/repos/$REPO/releases/latest"
RELEASE_JSON=$($DOWNLOADER "$RELEASE_URL" 2>/dev/null || echo "")

if [ -z "$RELEASE_JSON" ] || echo "$RELEASE_JSON" | grep -q "Not Found"; then
    # No releases yet — build from source or use dev build
    warn "No releases found. Installing from latest build..."
    VERSION="dev"
    USE_APPIMAGE=true
else
    VERSION=$(echo "$RELEASE_JSON" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
    ok "Latest version: ${BOLD}$VERSION${NC}"
    USE_APPIMAGE=false
fi

# ─── Install system dependencies ────────────────────────────────────────────

info "Installing system dependencies..."

install_deps_apt() {
    local deps="libwebkit2gtk-4.1-0 libayatana-appindicator3-1 librsvg2-2"
    local missing=""
    for dep in $deps; do
        if ! dpkg -s "$dep" &>/dev/null; then
            missing="$missing $dep"
        fi
    done
    if [ -n "$missing" ]; then
        sudo apt-get update -qq
        sudo apt-get install -y -qq $missing
        ok "Dependencies installed"
    else
        ok "Dependencies already satisfied"
    fi
}

install_deps_dnf() {
    sudo dnf install -y webkit2gtk4.1 libayatana-appindicator-gtk3 librsvg2 2>/dev/null
    ok "Dependencies installed"
}

install_deps_pacman() {
    sudo pacman -S --noconfirm --needed webkit2gtk-4.1 libayatana-appindicator librsvg 2>/dev/null
    ok "Dependencies installed"
}

case "$PKG_MANAGER" in
    apt)     install_deps_apt ;;
    dnf)     install_deps_dnf ;;
    pacman)  install_deps_pacman ;;
    *)       warn "Unknown package manager. You may need to install WebKit2GTK 4.1 manually." ;;
esac

# ─── Build from source (fallback) ────────────────────────────────────────────

build_from_source() {
    info "Installing build dependencies..."

    case "$PKG_MANAGER" in
        apt)
            sudo apt-get install -y -qq \
                libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
                patchelf libssl-dev pkg-config libudev-dev build-essential git 2>/dev/null
            ;;
        dnf)
            sudo dnf install -y webkit2gtk4.1-devel libayatana-appindicator-gtk3-devel \
                librsvg2-devel openssl-devel pkg-config systemd-devel git 2>/dev/null
            ;;
        pacman)
            sudo pacman -S --noconfirm --needed webkit2gtk-4.1 libayatana-appindicator \
                librsvg openssl pkgconf base-devel git 2>/dev/null
            ;;
    esac
    ok "Build dependencies installed"

    # Install Rust if needed
    if ! command -v rustc &>/dev/null; then
        info "Installing Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y 2>/dev/null
        source "$HOME/.cargo/env"
        ok "Rust $(rustc --version | cut -d' ' -f2) installed"
    fi

    # Install Node.js if needed
    if ! command -v node &>/dev/null; then
        info "Installing Node.js..."
        case "$PKG_MANAGER" in
            apt)
                curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash - 2>/dev/null
                sudo apt-get install -y -qq nodejs
                ;;
            dnf) sudo dnf install -y nodejs ;;
            pacman) sudo pacman -S --noconfirm nodejs npm ;;
        esac
        ok "Node.js $(node --version) installed"
    fi

    # Clone and build
    info "Cloning repository..."
    BUILD_DIR=$(mktemp -d)
    git clone --depth 1 https://github.com/$REPO.git "$BUILD_DIR/trellis" 2>/dev/null
    ok "Repository cloned"

    info "Building Trellis (this may take a few minutes)..."
    cd "$BUILD_DIR/trellis/app"
    npm ci --silent 2>/dev/null
    source "$HOME/.cargo/env" 2>/dev/null || true
    npx tauri build 2>/dev/null

    # Install the built package
    if [ -f src-tauri/target/release/bundle/deb/*.deb ] 2>/dev/null; then
        sudo dpkg -i src-tauri/target/release/bundle/deb/*.deb 2>/dev/null || sudo apt-get install -f -y -qq
        ok "Trellis installed from source (.deb)"
    elif ls src-tauri/target/release/bundle/appimage/*.AppImage 1>/dev/null 2>/dev/null; then
        sudo mkdir -p "$APP_DIR"
        sudo cp src-tauri/target/release/bundle/appimage/*.AppImage "$APP_DIR/Trellis.AppImage"
        sudo chmod +x "$APP_DIR/Trellis.AppImage"
        sudo ln -sf "$APP_DIR/Trellis.AppImage" "$INSTALL_DIR/trellis"
        ok "Trellis installed from source (AppImage)"
    else
        fail "Build produced no installable packages"
    fi

    rm -rf "$BUILD_DIR"
    # Skip the rest of the download/install section
    BUILT_FROM_SOURCE=true
}

# ─── Download and install Trellis ────────────────────────────────────────────

INSTALL_DIR="/usr/local/bin"
APP_DIR="/opt/trellis"
DESKTOP_FILE="/usr/share/applications/trellis.desktop"
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT
BUILT_FROM_SOURCE=false

if [ "$BUILT_FROM_SOURCE" = true ]; then
    : # Already installed during build
elif [ "$USE_APPIMAGE" = true ] || [ "$VERSION" = "dev" ]; then
    # AppImage installation (universal fallback)
    info "Downloading Trellis AppImage..."

    APPIMAGE_URL=$(echo "$RELEASE_JSON" | grep "browser_download_url" | grep "\.AppImage" | head -1 | sed 's/.*"\(https[^"]*\)".*/\1/')
    [ -z "$APPIMAGE_URL" ] && APPIMAGE_URL="https://github.com/$REPO/releases/latest/download/Trellis_${DEB_ARCH}.AppImage"

    if ! $DOWNLOADER "$APPIMAGE_URL" > "$TMP_DIR/Trellis.AppImage" 2>/dev/null || [ ! -s "$TMP_DIR/Trellis.AppImage" ]; then
        warn "No pre-built binary found. Building from source..."
        build_from_source
    fi

    sudo mkdir -p "$APP_DIR"
    sudo cp "$TMP_DIR/Trellis.AppImage" "$APP_DIR/Trellis.AppImage"
    sudo chmod +x "$APP_DIR/Trellis.AppImage"

    # Create symlink
    sudo ln -sf "$APP_DIR/Trellis.AppImage" "$INSTALL_DIR/trellis"

    ok "Trellis AppImage installed to $APP_DIR"

else
    # Package-based installation — find actual asset URL from API
    case "$PKG_MANAGER" in
        apt)
            DEB_URL=$(echo "$RELEASE_JSON" | grep "browser_download_url" | grep "\.deb" | head -1 | sed 's/.*"\(https[^"]*\)".*/\1/')
            if [ -z "$DEB_URL" ]; then
                warn "No .deb found in release. Falling back to AppImage."
                USE_APPIMAGE=true
            else
                info "Downloading Trellis .deb package..."
                $DOWNLOADER "$DEB_URL" > "$TMP_DIR/trellis.deb"
                sudo dpkg -i "$TMP_DIR/trellis.deb" 2>/dev/null || sudo apt-get install -f -y -qq
                ok "Trellis installed via .deb"
            fi
            ;;
        dnf)
            RPM_URL=$(echo "$RELEASE_JSON" | grep "browser_download_url" | grep "\.rpm" | head -1 | sed 's/.*"\(https[^"]*\)".*/\1/')
            if [ -z "$RPM_URL" ]; then
                warn "No .rpm found in release. Falling back to AppImage."
                USE_APPIMAGE=true
            else
                info "Downloading Trellis .rpm package..."
                $DOWNLOADER "$RPM_URL" > "$TMP_DIR/trellis.rpm"
                sudo rpm -i "$TMP_DIR/trellis.rpm" 2>/dev/null || sudo dnf install -y "$TMP_DIR/trellis.rpm"
                ok "Trellis installed via .rpm"
            fi
            ;;
        *)
            # Fall back to AppImage
            USE_APPIMAGE=true
            warn "Falling back to AppImage installation"
            ;;
    esac
fi

# ─── Create desktop entry ───────────────────────────────────────────────────

if [ ! -f "$DESKTOP_FILE" ]; then
    sudo tee "$DESKTOP_FILE" > /dev/null << 'DESKTOP'
[Desktop Entry]
Name=Trellis
Comment=ESP32 & Pico W Device Control Center
Exec=trellis
Icon=trellis
Type=Application
Categories=Development;Electronics;
Keywords=esp32;pico;iot;microcontroller;
StartupWMClass=Trellis
DESKTOP
    sudo update-desktop-database /usr/share/applications 2>/dev/null || true
    ok "Desktop entry created"
fi

# ─── Add user to dialout group (for serial port access) ─────────────────────

if ! groups | grep -q dialout; then
    sudo usermod -aG dialout "$USER" 2>/dev/null || true
    warn "Added you to 'dialout' group for serial port access."
    warn "You may need to ${BOLD}log out and back in${NC} for this to take effect."
else
    ok "Serial port access: dialout group"
fi

# ─── Optional: Arduino CLI ──────────────────────────────────────────────────

echo ""
if command -v arduino-cli &>/dev/null; then
    ok "Arduino CLI already installed: $(arduino-cli version 2>/dev/null | head -1)"
else
    ask "Install Arduino CLI for serial monitor & firmware flashing? (Y/n)"
    read -r response
    response=${response:-Y}
    if [[ "$response" =~ ^[Yy]$ ]]; then
        info "Installing Arduino CLI..."
        ARDUINO_BIN="${HOME}/.local/bin"
        mkdir -p "$ARDUINO_BIN"
        curl -fsSL https://raw.githubusercontent.com/arduino/arduino-cli/master/install.sh | BINDIR="$ARDUINO_BIN" sh 2>/dev/null
        ok "Arduino CLI installed to $ARDUINO_BIN"

        # Install ESP32 core
        ask "Install ESP32 board support? (Y/n)"
        read -r response2
        response2=${response2:-Y}
        if [[ "$response2" =~ ^[Yy]$ ]]; then
            info "Installing ESP32 core (this may take a minute)..."
            "$ARDUINO_BIN/arduino-cli" core update-index 2>/dev/null
            "$ARDUINO_BIN/arduino-cli" core install esp32:esp32 2>/dev/null
            ok "ESP32 board support installed"
        fi
    else
        ok "Skipping Arduino CLI"
    fi
fi

# ─── Done ────────────────────────────────────────────────────────────────────

echo ""
echo -e "${GREEN}${BOLD}  ✨ Trellis is ready!${NC}"
echo ""
echo -e "  Launch from your app menu, or run:  ${BOLD}trellis${NC}"
echo ""
echo -e "  ${DIM}GitHub:  https://github.com/ovexro/trellis${NC}"
echo -e "  ${DIM}Donate:  https://www.paypal.com/paypalme/ovexro${NC}"
echo ""
