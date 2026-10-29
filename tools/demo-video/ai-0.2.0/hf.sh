#!/bin/bash
# hf.sh <hyperframes args>: run the pinned HyperFrames (0.8.153) in the composition project
M="$HOME/side projects/rusty-inbox-video-media"
cd "$M/video" && exec node --max-old-space-size=6144 "$M/hf/node_modules/hyperframes/bin/hyperframes.mjs" "$@"
