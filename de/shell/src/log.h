// The shell's log category: quiet by default; QT_LOGGING_RULES=
// "fern.shell.info=true" (the session sets it) shows what it does
// (screenshots, notifications, windows opening; never their contents), in
// fern-comp's log.
#pragma once

#include <QLoggingCategory>

Q_DECLARE_LOGGING_CATEGORY(lcShell)
