# Bonnet
Guide here: https://learn.adafruit.com/adafruit-voice-bonnet/raspberry-pi-setup

# WARNING
Ubuntu 21.04 has an annoying whine when recording audio! It seems to be working on 21.10

# On the PI
```bash
git clone https://github.com/HinTak/seeed-voicecard
cd seeed-voicecard
sudo ./install.sh
sudo reboot now

# Set volume
CARD_ID="$(aplay -l | grep seeed | cut -d ' ' -f 2 | cut -d ':' -f 1)" && amixer -c "$CARD_ID" sset Speaker "79%"

# Test speaker
CARD_ID="$(aplay -l | grep seeed | cut -d ' ' -f 2 | cut -d ':' -f 1)" && amixer -c "$CARD_ID" sset Speaker "79%" && speaker-test -c2 -Dhw:"$CARD_ID"

# Test mic
CARD_ID="$(aplay -l | grep seeed | cut -d ' ' -f 2 | cut -d ':' -f 1)" && amixer -c "$CARD_ID" sset Speaker "79%" && sudo arecord -c2 -f S16_LE -r 16000 --device="hw:$CARD_ID,0" | aplay -Dhw:"$CARD_ID"

# Tell boxen-client which devices to use 
echo "alsasrc device=\"hw:$CARD_ID\"" > input.txt
echo "alsasink device=\"hw:$CARD_ID\"" > output.txt

# If that doesn't work:
echo "audio/x-raw,format=S16LE,rate=48000 ! alsasink device=\"hw:$CARD_ID\"" > output.txt
```