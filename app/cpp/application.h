#pragma once

#include <memory>

#include <QtGui/QGuiApplication>

#include "rust/cxx.h"

// The application object is a QApplication (Qt Widgets), not a bare QGuiApplication. Some desktop
// platform themes build widgets behind Qt.labs.platform and QtQuick.Dialogs: on KDE Plasma the tray
// menu is a QMenu and the file dialogs are KDE widgets, and a QGuiApplication aborts with "Cannot create
// a QWidget without QApplication". Returned as its QGuiApplication base so the cxx-qt-lib API applies.
std::unique_ptr<QGuiApplication> rustyinbox_new_application(rust::Slice<const rust::String> args);
