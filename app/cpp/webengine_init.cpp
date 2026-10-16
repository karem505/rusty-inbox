#include "webengine_init.h"

#include <QtCore/QCoreApplication>
#include <QtWebEngineQuick/qtwebenginequickglobal.h>

void rustyinbox_init_webengine()
{
    QCoreApplication::setAttribute(Qt::AA_ShareOpenGLContexts);
    QtWebEngineQuick::initialize();
}
