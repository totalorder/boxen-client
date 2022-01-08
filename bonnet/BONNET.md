# Bonnet
Guide here: https://learn.adafruit.com/adafruit-voice-bonnet/raspberry-pi-setup

## Copy stuff to PI
```bash
scp bonnet/raspi-blinka.py "$(cat pi-ip.txt):bonnet/"
ssh "$(cat pi-ip.txt)"
```

# On the PI
```bash
sudo apt install -y python3 python3-venv i2c-tools
```

## Try the adafruit bonnet stuff
```
mkdir ~/bonnet
cd ~/bonnet
python3 -m venv .venv --prompt bonnet
source .venv/bin/activate
python raspi-blinka.py
```

## Try the other stuff
```
git clone https://github.com/HinTak/seeed-voicecard
cd seeed-voicecard
sudo ./install.sh
sudo reboot now
alsamixer # Press F6 and selec seeed, select 100<>100 Speaker, set volume to 50

CARD_ID="$(sudo aplay -l | grep seeed | cut -d ' ' -f 2 | cut -d ':' -f 1)"
speaker-test -c2 -Dhw:$CARD_ID

sudo arecord -c2 -f S16_LE -r 16000 --device="hw:$CARD_ID,0" | aplay -Dhw:$CARD_ID
```