# Setup on PC
Download, install and start rpi-imager
```bash
wget -q https://downloads.raspberrypi.org/imager/imager_latest_amd64.deb
sudo apt install -y ./imager_latest_amd64.deb
./rpi-imager
```

Follow the instructions and select: Ubuntu > Ubuntu Desktop 21.04 (64-bit)
Install Ubnutu on the SD-card and boot the Pi

# Setup on Pc & PI
## (on pc) Install packages
Install all the things
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

## (on pi) Install packages
Install all the things
```bash
sudo apt update && sudo apt upgrade -y && apt install -y \
  openssh-server \
  gstreamer1.0-tools gstreamer1.0-nice gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-plugins-good \
  ninja-build \
  python3-pip \
  libssl-dev \
  git
```

## (on pc and pi) Install meson
```bash
pip3 install --user meson
echo -e 'PATH="$PATH:$HOME/.local/bin"\n' >> ~/.bashrc
source ~/.bashrc
```

## (on pc) Install rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

echo -e 'source $HOME/.cargo/env\n' >> ~/.bashrc
source ~/.bashrc
```

## (on pc) Install support for cross-compiling to raspberry pi
```bash
cargo install cross
```

### (on pc) Install docker
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

### (on pc) Build cross images
```bash
(cd cross-image && ./build.sh)
```

## (on pc and pi) Clone and build libnice
```bash
git clone https://gitlab.freedesktop.org/libnice/libnice.git

cd libnice
meson --prefix "$(pwd)/install" builddir
ninja -C builddir
ninja -C builddir install
cd install/lib/aarch64-linux-gnu/
echo "$(pwd)" > ~/.libnice-library-path.txt
```

## (on pc) Clone the repo and setup audio
Clone the repo and check out branch "test1"
```bash
git clone --recursive https://github.com/totalorder/boxen-client.git
cd boxen-client/
```

Figure out how to access your microphone and speakers from gstreamer
```bash
gst-device-monitor-1.0
```

(on pc and pi) Put the command for the microphone in "input.txt"
```bash
# NOTE: This is just an example
echo "pulsesrc device=alsa_input.usb-046d_09a1_2560E220-02.mono-fallback" > input.txt
echo "autoaudiosrc" > input.txt
```

(on pc and pi) Put the command for the headphones/speakers in "output.txt"
```bash
# NOTE: This is just an example
echo "autoaudiosink" > output.txt
```

(pc only) Add the IP of the pi to pi-ip.txt and remote gst-examples path to gst-examples-dir.txt"
```bash
# NOTE: This is just an example
echo "192.168.2.203" > pi-pi.txt
echo "projects/gst-examples" > gst-examples-dir.txt
``` 

(pc only) Add yourself to authorized keys on the pi
```bash
ssh $(cat pi-ip.txt) mkdir ~/.ssh && scp ~/.ssh/id_*.pub "$(cat pi-ip.txt):.ssh/authorized_keys"
```
 
(pc only) Compile and run the application on both pc and pi.
```bash
./run-both
```
