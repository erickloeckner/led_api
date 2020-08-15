API for controlling addressable LEDs connected to a Raspberry Pi's SPI port/s

# building:
```
cargo +nightly build --release
```

### install nightly toolchain:
```
rustup toolchain install nightly
```

### build and restart service:
```
cargo +nightly build --release && systemctl --user restart led-api.service
```

# systemd service:

### first enable persistent user systemd services:
```
loginctl enable-linger pi
```
### create directory and service file:
```
mkdir -p /home/pi/.config/systemd/user/

touch /home/pi/.config/systemd/user/led-api.service
```
### contents of service file:
```
cat /home/pi/.config/systemd/user/led-api.service
[Unit]
Description=LED API

[Service]
ExecStart=/home/pi/rust/led_api/target/release/led_api

[Install]
WantedBy=default.target
```

### start and enable service:
```
systemctl --user start led-api.service

systemctl --user enable led-api.service
```
