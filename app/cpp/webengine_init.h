#pragma once

// QtWebEngine must be initialised before the QGuiApplication exists (stock Qt API, no patching).
void rustyinbox_init_webengine();
