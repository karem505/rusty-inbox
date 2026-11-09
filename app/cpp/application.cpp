#include "application.h"

#include <string>
#include <vector>

#include <QtWidgets/QApplication>

std::unique_ptr<QGuiApplication> rustyinbox_new_application(rust::Slice<const rust::String> args)
{
    // QApplication keeps references to argc and argv for its whole life, so they live in statics.
    static std::vector<std::string> storage;
    static std::vector<char*> argv;
    static int argc = 0;
    storage.clear();
    for (const auto& a : args)
        storage.emplace_back(std::string(a));
    argv.clear();
    for (auto& s : storage)
        argv.push_back(s.data());
    argv.push_back(nullptr);
    argc = static_cast<int>(storage.size());
    return std::make_unique<QApplication>(argc, argv.data());
}
