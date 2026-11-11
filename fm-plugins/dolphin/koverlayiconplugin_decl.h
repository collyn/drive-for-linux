// Declaration of KIO's KOverlayIconPlugin (KF6 KIOCore, koverlayiconplugin.h), written out here
// because the KIO development headers aren't installed. Its implementation (and its meta object)
// comes from libKF6KIOCore.so.6; the class adds no data to QObject (its deleting destructor frees
// sizeof(QObject) = 16 bytes) and one pure virtual after QObject's.
#pragma once
#include <QtCore/QObject>
#include <QtCore/QStringList>
#include <QtCore/QUrl>

class KOverlayIconPlugin : public QObject
{
    Q_OBJECT
public:
    explicit KOverlayIconPlugin(QObject *parent = nullptr);
    ~KOverlayIconPlugin() override;
    virtual QStringList getOverlays(const QUrl &item) = 0;
Q_SIGNALS:
    void overlaysChanged(const QUrl &url, const QStringList &overlays);
};
