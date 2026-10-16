#!/bin/bash
# The Qt SDK (/usr) has no QtWebEngine; the base app ships it under /app without .prl files.
# cxx-qt-build asks `qmake -query` for one lib/include dir, so present a merged view of both
# through a qmake wrapper. Usage: qmake-webengine-overlay.sh <overlay-dir>; then QMAKE=<overlay-dir>/bin/qmake
set -euo pipefail
O=$1
mkdir -p "$O/lib" "$O/include" "$O/bin"
REAL_LIBS=$(/usr/bin/qmake -query QT_INSTALL_LIBS)
REAL_INC=$(/usr/bin/qmake -query QT_INSTALL_HEADERS)
for f in "$REAL_LIBS"/libQt6*; do ln -sfn "$f" "$O/lib/"; done
for f in /app/lib/libQt6WebEngine*; do ln -sfn "$f" "$O/lib/"; done
for d in "$REAL_INC"/Qt*; do ln -sfn "$d" "$O/include/"; done
for d in /app/include/QtWebEngine* /app/include/QtPdf*; do [ -e "$d" ] && ln -sfn "$d" "$O/include/"; done
for m in WebEngineCore WebEngineQuick; do
  cat > "$O/lib/libQt6$m.prl" <<P
QMAKE_PRL_TARGET = libQt6$m.so
QMAKE_PRL_CONFIG = shared
QMAKE_PRL_VERSION = 6.11
QMAKE_PRL_LIBS = \$\$[QT_INSTALL_LIBS]/libQt6WebEngineCore.so \$\$[QT_INSTALL_LIBS]/libQt6Quick.so \$\$[QT_INSTALL_LIBS]/libQt6Gui.so \$\$[QT_INSTALL_LIBS]/libQt6Qml.so \$\$[QT_INSTALL_LIBS]/libQt6Core.so
P
done
cat > "$O/bin/qmake" <<W
#!/bin/bash
# \`qmake -query VAR\` prints the bare value, \`qmake -query\` prints VAR:value lines
case "\$*" in
  "-query QT_INSTALL_LIBS") echo "$O/lib" ;;
  "-query QT_INSTALL_HEADERS") echo "$O/include" ;;
  *) /usr/bin/qmake "\$@" | sed -e 's#^QT_INSTALL_LIBS:.*#QT_INSTALL_LIBS:$O/lib#' -e 's#^QT_INSTALL_HEADERS:.*#QT_INSTALL_HEADERS:$O/include#' ;;
esac
W
chmod +x "$O/bin/qmake"
