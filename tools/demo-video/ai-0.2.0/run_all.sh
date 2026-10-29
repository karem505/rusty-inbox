#!/bin/bash
# fresh data, app start, wait for the background AI, then record every beat
cd "$(dirname "$0")"; . ./env.sh
timeout 580 python3 prep.py < /dev/null || exit 1
../demo-env.sh x; ../demo-env.sh app
sleep ${WAIT:-110}
python3 record.py rec ${@:-settings write read attach safe ontop privacy}
