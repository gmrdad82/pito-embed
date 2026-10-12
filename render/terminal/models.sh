#!/bin/sh
set -eu
app=$1
demo=$2
src=${PITO_EMBED_MODELS:-$app/tmp/models/embeddinggemma-300m-q4}
mkdir -p "$demo"
ln -sfn "$(cd "$src" && pwd)" "$demo/model"
