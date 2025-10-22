# Dashcam (Rust rewrite)
- cargo build --release
    - for V4l2 based drivers (often Linux systems with a usb camera)
- cargo build --release --features rpi
    - for Libcamera based drivers, i.e. Raspberry Pi Camera systems.

# Original README from C++:
## 📹 Dashcam

### 📄 Description

This project is a **dashcam application** that allows users to record and store video footage while driving.

🌐 A **web interface** supports real-time HTTP Live Stream and historical video playback.

See also: [Dashcam Web](https://github.com/fjh32/dashcam_web)

📡 For an optimal experience, the Raspberry Pi should be set up as a **Wireless Access Point**.

---

### 📦 Build Dependencies

#### On a new Raspberry Pi, install these packages:

```sh
sudo apt install cmake libcamera-dev libjsoncpp-dev uuid-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev gstreamer1.0-libcamera libgstreamer-plugins-bad1.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav gstreamer1.0-tools gstreamer1.0-x
```

- C++ GStreamer libraries
- `libcamera` for Raspberry Pi Camera Module
- `cmake` build system

---

### ▶️ Running

-  Make sure to have `cmake` and an up-to-date C++ compiler

1.  Clone the repository
2. 🚀 Run: `./build_and_install.sh`
   -  Recordings directory: `/var/lib/dashcam/recordings`
   -  `dashcam` binary is installed to: `/usr/local/bin`
   -  `dashcam.service` is installed to: `/etc/systemd/system`

---

###  Features

- Real-time video recording
- Clip saving and automatic file cleanup
- Video playback
- TODO: Automatic event detection
- ⚙TODO: Customizable settings

---

### 🌐 Web Interface

A separate GitHub repo will host the web interface for real-time video playback and controls.

---

### 🧪 Dev Usage

- Pipe interface: `/tmp/camrecorder.pipe`
- Save last 300 seconds:
  ```sh
  echo save:300 >> /tmp/camrecorder.pipe
  ```
- Stop the server:
  ```sh
  echo stop >> /tmp/camrecorder.pipe
  ```
- Press `Ctrl+C` to kill the server cleanly.

---

### 🤝 Contributing

Contributions are welcome! Please follow the guidelines in the [CONTRIBUTING.md](./CONTRIBUTING.md) file.

