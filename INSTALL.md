# Setup on PC
## Un Ubuntu 21.04 or later: Install gstreamer and other packages 
```bash
sudo apt update && sudo apt upgrade -y && sudo apt install -y \
  vim \
  git \
  libssl-dev \
  gstreamer1.0-tools gstreamer1.0-nice gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-plugins-good libgstreamer1.0-dev git libglib2.0-dev libgstreamer-plugins-bad1.0-dev libsoup2.4-dev libjson-glib-dev \
  curl \
  build-essential \
  openssh-server \
  ninja-build \
  python3-pip
```

### On Ubuntu 20.04 or earlier: Build gstreamer 1.18 from source
```
# Install required packages
sudo apt update && sudo apt upgrade -y && sudo apt install -y \
  vim \
  git \
  libssl-dev \
  git \
  libglib2.0-dev \
  ninja-build \
  libbison-dev \
  flex \
  libcairo2-dev \
  libmount-dev \
  libopus-dev \
  libsrtp2-dev \
  libvpx-dev \
  python3-pip

# Clone the gst-build project, checkout the right branch, set up a virtualenv and install the meson build system
git clone https://gitlab.freedesktop.org/gstreamer/gst-build.git
cd gst-build
git checkout 1.18.5
python3 -m venv .venv
source .venv/bin/activate
pip install meson

# Build gstreamer from source and install it in a local folder
meson --prefix="$(pwd)/install" -Dbad=enabled -Dgst-plugins-bad:webrtc=enabled -Dgst-plugins-base:opus=enabled builddir
ninja -C builddir
meson install -C builddir

# Tell the boxen-client build tools where our custom built gstreamer lives
(cd install/lib/x86_64-linux-gnu/ && echo "$(pwd)" > ~/.libgstreamer-library-path.txt)
```

## Install rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

echo -e 'source $HOME/.cargo/env\n' >> ~/.bashrc
source ~/.bashrc
```

### Install rust cross-compiling support
```bash
cargo install cross
```

### Install docker
Docker is a prerequisite for using cross
```bash
sudo apt-get update
sudo apt-get install \
    apt-transport-https \
    ca-certificates \
    curl \
    gnupg \
    lsb-release
curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo gpg --dearmor -o /usr/share/keyrings/docker-archive-keyring.gpg
echo \
  "deb [arch=amd64 signed-by=/usr/share/keyrings/docker-archive-keyring.gpg] https://download.docker.com/linux/ubuntu \
  $(lsb_release -cs) stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
sudo apt-get update
sudo apt-get install docker-ce docker-ce-cli containerd.io  
sudo groupadd docker
sudo usermod -aG docker $USER
newgrp docker 
```

### Build cross images
```bash
(cd cross-image && ./build.sh)
```

## Compile libnice
The version of libnice available in the apt-repos has a bug which makes it crash. Build the latest version from source instead.

### Install meson
Meson is a prerequisite for building libnice
```bash
pip3 install --user meson
echo -e 'PATH="$PATH:$HOME/.local/bin"\n' >> ~/.bashrc
source ~/.bashrc
```

### Clone and build libnice
```bash
git clone https://gitlab.freedesktop.org/libnice/libnice.git

cd libnice
meson --prefix "$(pwd)/install" builddir
ninja -C builddir
ninja -C builddir install
cd install/lib/aarch64-linux-gnu/
# Put the path of the compiled libnice in a file in the home directory for later use
echo "$(pwd)" > ~/.libnice-library-path.txt
```

## Clone the boxen-client repo
Clone the repo and cd into it
```bash
git clone --recursive https://github.com/totalorder/boxen-client.git
cd boxen-client/
```

### Configure audio inputs and outputs
Figure out how to access your microphone and speakers from gstreamer
```bash
gst-device-monitor-1.0
```

Put the command for the microphone in "input.txt"
```bash
# NOTE: This is just an example
echo "autoaudiosrc" > input.txt
# or
echo "pulsesrc device=alsa_input.usb-046d_09a1_2560E220-02.mono-fallback" > input.txt
```

Put the command for the headphones/speakers in "output.txt"
```bash
# NOTE: This is just an example
echo "autoaudiosink" > output.txt
```

# Set up the Pi

## Prepare the SD-card
Download, install and start rpi-imager
```bash
wget -q https://downloads.raspberrypi.org/imager/imager_latest_amd64.deb
sudo apt install -y ./imager_latest_amd64.deb
```

Start rpi-imager and follow the instructions. Select: Ubuntu > Ubuntu Desktop 21.04 (64-bit)
Install Ubuntu on the SD-card and boot the Pi
```bash
rpi-imager
```

## Set up ssh
Boot the Pi and connect to the network, then install the ssh server on the Pi
```bash
sudo apt install -y openssh-server
```

Find the ip of the Pi
```bash
ip route show
```

On your PC, put the ip-address of the Pi in a file called pi-ip.txt in the root of the boxen-client repo
```bash
echo "192.168.0.123" > pi-ip.txt
```

On your PC, scp your public key to the Pi, adding it as an authorized key on the Pi
```bash
ssh $(cat pi-ip.txt) mkdir .ssh && scp ~/.ssh/id_*.pub "$(cat pi-ip.txt):.ssh/authorized_keys"
```
### Connect to the Pi over ssh
On your PC, connect to the Pi over ssh so that the setup can be completed from your PC
```bash
ssh $(cat pi-ip.txt)
```
All subsequent commands should be run on the Pi

## Install packages
```bash
sudo apt update && sudo apt upgrade -y && sudo apt install -y \
  gstreamer1.0-tools gstreamer1.0-nice gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-plugins-good \
  libssl-dev \
  ninja-build \
  python3-pip \
  git
```

## Allow non-root access to the GPIO-pins on the Pi
```bash
sudo adduser $USER dialout

# If on Ubuntu 21.10. Make sure the "dialout" user get ownership on startup
scp 99-gpio.rules $(cat pi-ip.txt):. && ssh -t $(cat pi-ip.txt) "sudo cp 99-gpio.rules /usr/lib/udev/rules.d/99-gpio.rules"
 
#sudo chown root.dialout /dev/gpiomem && sudo chmod g+rw /dev/gpiomem
#sudo chown root.dialout /dev/gpiochip0 && sudo chmod g+rw /dev/gpiochip0

# Reboot the Pi so that systemd discovers the new group on startup 
sudo reboot now
```

## Compile libnice
The version of libnice available in the apt-repos has a bug which makes it crash. Build the latest version from source instead.

### Install meson
Meson is a prerequisite for building libnice
```bash
pip3 install --user meson
echo -e 'PATH="$PATH:$HOME/.local/bin"\n' >> ~/.bashrc
source ~/.bashrc
```

### Clone and build libnice
```bash
git clone https://gitlab.freedesktop.org/libnice/libnice.git

cd libnice
meson --prefix "$(pwd)/install" builddir
ninja -C builddir
ninja -C builddir install
cd install/lib/aarch64-linux-gnu/
# Put the path of the compiled libnice in a file in the home directory for later use
echo "$(pwd)" > ~/.libnice-library-path.txt
```

## Configure audio inputs and outputs
Figure out how to access your microphone and speakers from gstreamer
```bash
gst-device-monitor-1.0
```

Put the command for the microphone in "input.txt"
```bash
# NOTE: This is just an example
echo "autoaudiosrc" > input.txt
```

Put the command for the headphones/speakers in "output.txt"
```bash
# NOTE: This is just an example
echo "autoaudiosink" > output.txt
```

# Run the application
On the pc, compile and run the application on both pc and pi.
```bash
./run-both
```

## Install boxen-client as a systemd service
This will start boxen-client on boot
```bash
(cd systemd && ./install $(cat ../pi-ip.txt))
```


# Make sure the server is running
https://github.com/totalorder/gst-examples-copy/blob/master/webrtc/signalling/Dockerfile