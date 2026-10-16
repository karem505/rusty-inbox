#!/bin/bash
# reset.sh: fresh demo mailbox and theme, then an empty private display (the app is not started).
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
D=${DEMO_DIR:-$ROOT/target/demo}
"$ROOT/tools/demo-video/demo-env.sh" stop > /dev/null
rm -rf "$D/core-data" "$D/theme" "$D/shots"
cd "$ROOT" && cargo run -q --release -p rusty-inbox-core --example seed_demo -- "$D/core-data" < /dev/null
"$ROOT/tools/demo-video/demo-env.sh" x
