# Configure Wi-Fi with a USB stick

On a USB-stick, create a file called `wifi.txt` with your Wi-Fi's name and password on the first and second line, like so:

`wifi.txt`
```
ssid-here
password-here
```

When inserted usbwifi will automatically mount the USB-stick, read the SSID and password from `wifi.txt`, and configure
`/etc/netplan/99-custom-network.yaml` to connect to the Wi-Fi.
