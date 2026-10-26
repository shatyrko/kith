#!/usr/bin/env bash
# Builds Kith and installs it for your user: ~/.local/bin/kith, an icon and a launcher entry.
# Needs: rust (cargo), node + npm, and the Tauri 2 system libraries (webkit2gtk-4.1, gtk3, libsoup3).
set -euo pipefail
cd "$(dirname "$0")/.."
[[ -d node_modules ]] || npm install --no-audit --no-fund
npm run build
cargo build --release -p kith --features custom-protocol
install -Dm755 target/release/kith "$HOME/.local/bin/kith"
install -Dm644 src-tauri/icons/128x128.png "$HOME/.local/share/icons/hicolor/128x128/apps/kith.png"
install -Dm644 src-tauri/icons/128x128@2x.png "$HOME/.local/share/icons/hicolor/256x256/apps/kith.png"
mkdir -p "$HOME/.local/share/applications"
cat > "$HOME/.local/share/applications/dev.alecsh.Kith.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Kith
GenericName=Contacts
Comment=A modern address book that syncs with Google Contacts
Exec=$HOME/.local/bin/kith
Icon=kith
Categories=Office;ContactManagement;
StartupWMClass=kith
Keywords=contacts;address book;people;vcard;google;
MimeType=text/vcard;text/x-vcard;
DESKTOP
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "Installed: $HOME/.local/bin/kith"
