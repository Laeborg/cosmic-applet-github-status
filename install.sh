#!/bin/bash
set -e

REPO="Laeborg/cosmic-applet-github-status"
APPID="com.laeborg.CosmicAppletGithubStatus"
BIN_NAME="cosmic-applet-github-status"
BASE_URL="https://github.com/$REPO"
RAW_URL="https://raw.githubusercontent.com/$REPO/main"

echo "Installing $BIN_NAME..."

# Binary from latest release
curl -fsSL "$BASE_URL/releases/latest/download/$BIN_NAME" -o "/tmp/$BIN_NAME"
sudo install -Dm0755 "/tmp/$BIN_NAME" "/usr/local/bin/$BIN_NAME"
rm "/tmp/$BIN_NAME"

# Desktop file
curl -fsSL "$RAW_URL/resources/app.desktop" \
  | sudo tee "/usr/local/share/applications/$APPID.desktop" > /dev/null

# Icon
sudo mkdir -p "/usr/local/share/icons/hicolor/scalable/apps"
curl -fsSL "$RAW_URL/resources/icon.svg" \
  | sudo tee "/usr/local/share/icons/hicolor/scalable/apps/$APPID.svg" > /dev/null

# Update icon cache
gtk-update-icon-cache -f -t /usr/local/share/icons/hicolor/ 2>/dev/null || true

echo "Done! Add the applet to your panel:"
echo "  Edit panel → + → GitHub Status"
