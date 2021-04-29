#!/usr/bin/env bash
set -o errexit
set -o nounset
set -o pipefail
set -o xtrace

if [ ! -d cross ]; then
  git clone https://github.com/rust-embedded/cross.git
fi

cp Dockerfile.aarch64-unknown-linux-gnu-21-04-base cross/docker/
(cd cross/docker && docker build -t aarch64-unknown-linux-gnu-21-04-base -f Dockerfile.aarch64-unknown-linux-gnu-21-04-base .)
docker build -t aarch64-unknown-linux-gnu-21-04-boxen-client -f Dockerfile.aarch64-unknown-linux-gnu-21-04-boxen-client .
