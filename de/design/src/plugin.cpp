// The Fern.Design plugin: registers the symbol and adaptive icon image
// providers with every engine that imports the module.
#include <QQmlEngine>
#include <QQmlEngineExtensionPlugin>

#include "icons.h"
#include "symbolprovider.h"

extern void qml_register_types_Fern_Design();
Q_GHS_KEEP_REFERENCE(qml_register_types_Fern_Design)

class FernDesignPlugin : public QQmlEngineExtensionPlugin
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID QQmlEngineExtensionInterface_iid)

public:
    explicit FernDesignPlugin(QObject *parent = nullptr)
        : QQmlEngineExtensionPlugin(parent)
    {
        volatile auto registration = &qml_register_types_Fern_Design;
        Q_UNUSED(registration)
    }

    void initializeEngine(QQmlEngine *engine, const char *uri) override
    {
        Q_UNUSED(uri)
        if (!engine->imageProvider(QStringLiteral("symbol")))
            engine->addImageProvider(QStringLiteral("symbol"), new SymbolProvider);
        if (!engine->imageProvider(QStringLiteral("fernicon")))
            engine->addImageProvider(QStringLiteral("fernicon"), new IconProvider);
    }
};

#include "plugin.moc"
