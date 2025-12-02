#!/bin/bash

MAIN_DIR="/var/lib/dashcam"
RECORDINGS_DIR="$MAIN_DIR/recordings/"
REAL_USER=$(logname)

echo "Creating recordings directory at $RECORDINGS_DIR..."
sudo mkdir -p "$RECORDINGS_DIR"
sudo chown -R "$USER:$USER" "$MAIN_DIR"

echo "Building..."
cargo build --release --features rpi -j 1

echo "📦 Installing systemd service..."
sed "s|@USER@|$REAL_USER|g" dashcam_rs.service.template | sudo tee /etc/systemd/system/dashcam_rs.service > /dev/null

echo "📦 Copying Database Stuff to $MAIN_DIR..."
cp migrations/* $MAIN_DIR



echo "📦 Stopping existing services..."
sudo systemctl daemon-reload
sudo systemctl stop dashcam.service
sudo systemctl disable dashcam.service
sudo systemctl stop dashcam_rs.service

echo "📦 Installing binary to /usr/local/bin..."
sudo cp target/release/dashcam_rs /usr/local/bin/

# Reload and start systemd service
echo "📦 Starting dashcam_rs services..."
sudo systemctl enable dashcam_rs.service
sudo systemctl restart dashcam_rs.service

echo
echo "📦 Installation complete."
echo "You can check the status of the service with:"
echo "  sudo systemctl status dashcam_rs.service"
echo
