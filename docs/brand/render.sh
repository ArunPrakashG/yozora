#!/bin/sh
# Renders the brand images from src/ with headless Chrome, at 2x.
set -e
cd "$(dirname "$0")"
shot() { # name width height
  google-chrome-stable --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
    --default-background-color=00000000 --virtual-time-budget=3000 \
    --window-size="$2,$3" --screenshot="$PWD/$1.png" "file://$PWD/src/$1.html" 2>/dev/null
}
shot banner 1280 400
shot social 1280 640
shot poster 1200 1600
shot mark 512 512
