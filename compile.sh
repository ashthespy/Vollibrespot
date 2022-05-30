#!/bin/bash

# install dependencies 
apt-get update 
apt-get install -y build-essential libasound2-dev

# install rust 
curl https://sh.rustup.rs -sSf | sh
source $HOME/.cargo/env

# compile , substitue XXXXXXXXXXXX with spotify client id 
env CLIENT_ID="XXXXXXXXXXXX" cargo build --release

# copy in place 
# killall vollibrespot
# sudo cp -rp vollibrespot /usr/bin/vollibrespot
