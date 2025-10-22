#!/bin/bash

MAIN_DIR="/var/lib/dashcam"
RECORDINGS_DIR="$MAIN_DIR/recordings/"
REAL_USER=$(logname)

echo "Creating recordings directory at $RECORDINGS_DIR..."
sudo mkdir -p "$RECORDINGS_DIR"
sudo chown -R "$USER:$USER" "$RECORDINGS_DIR"

echo "Building..."
cargo build --release

echo "📝 Installing systemd service..."
sed "s|@USER@|$REAL_USER|g" dashcam_rs.service.template | sudo tee /etc/systemd/system/dashcam_rs.service > /dev/null
sudo sed -i '/^\[Service\]/a RestartSec=10s' /etc/systemd/system/dashcam_rs.service

echo "📦 Installing binary to /usr/local/bin..."
sudo cp target/release/dashcam_rs /usr/local/bin/

# Reload and start systemd service
echo "🔄 Reloading and enabling service..."
sudo systemctl daemon-reload
sudo systemctl stop dashcam.service
sudo systemctl disable dashcam.service
sudo systemctl enable dashcam.service
sudo systemctl restart dashcam.service

echo
echo "✅ Installation complete."
echo "You can check the status of the service with:"
echo "  sudo systemctl status dashcam.service"
echo
