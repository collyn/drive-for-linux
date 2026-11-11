// Dolphin overlay plugin: asks the running Drive For Linux app for each file's sync status and
// shows it as an emblem (like the original's Finder badges).
#include "koverlayiconplugin_decl.h"
#include <QtCore/QDateTime>
#include <QtCore/QDir>
#include <QtCore/QHash>
#include <QtCore/QTimer>
#include <QtNetwork/QLocalSocket>

namespace {
QString socketPath()
{
    QString dir = QString::fromLocal8Bit(qgetenv("XDG_RUNTIME_DIR"));
    if (dir.isEmpty())
        dir = QDir::tempPath();
    return dir + QStringLiteral("/drive-linux-") + QString::fromLocal8Bit(qgetenv("USER")) + QStringLiteral("-status.sock");
}

QStringList iconsFor(const QString &badge)
{
    if (badge.isEmpty())
        return {};
    // No "-" in the name: KDE would shorten an unknown dashed name and pick another icon.
    return {QStringLiteral("drivelinux_") + badge};
}
} // namespace

class DriveLinuxOverlay : public KOverlayIconPlugin
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID "org.drivelinux.overlayiconplugin" FILE "drivelinux_overlay.json")
public:
    DriveLinuxOverlay()
    {
        m_watch = new QLocalSocket(this);
        connect(m_watch, &QLocalSocket::connected, this, [this] {
            m_watch->write("WATCH\n");
            // Back in touch with the app (it restarts when updated): a new run of tries starts,
            // and the badges taken down while it was away are put back.
            m_retries = 0;
            m_quietUntil = 0;
            refresh();
        });
        connect(m_watch, &QLocalSocket::readyRead, this, [this] {
            m_watch->readAll();
            refresh();
        });
        m_retryTimer = new QTimer(this);
        m_retryTimer->setSingleShot(true);
        m_retryTimer->setInterval(kAskAgainMs);
        connect(m_retryTimer, &QTimer::timeout, this, &DriveLinuxOverlay::refresh);
        connect(m_watch, &QLocalSocket::disconnected, this, &DriveLinuxOverlay::lost);
        connect(m_watch, &QLocalSocket::errorOccurred, this, [this](QLocalSocket::LocalSocketError) {
            if (m_watch->state() == QLocalSocket::UnconnectedState)
                lost();
        });
        watch();
    }

    QStringList getOverlays(const QUrl &url) override
    {
        if (!url.isLocalFile())
            return {};
        QString path = url.toLocalFile();
        while (path.size() > 1 && path.endsWith(QLatin1Char('/')))
            path.chop(1);
        remember(url);
        const QStringList answer = query({path});
        if (answer.size() != 1) {
            // No answer now (the app is starting or busy): ask again shortly and show the badge then.
            m_known.insert(url, QString());
            m_retryTimer->start();
            return {};
        }
        m_known.insert(url, answer[0]);
        return iconsFor(answer[0]);
    }

private:
    // The original's listener (FinderSyncExtension 0x1000244d0 / 0x100024b30): when the link to the
    // app is lost every badge is taken down ("unpublish"), then it tries again after 5 s, counting
    // each try, and gives up once 6 tries were made ("Max retry count exceeded").
    static constexpr int kRetryDelayMs = 5000;
    static constexpr int kMaxRetries = 6;
    // Items not answered are asked again after this long (rebuild choice).
    static constexpr int kAskAgainMs = 2000;
    // Items remembered for updates; the oldest are forgotten past this many.
    static constexpr int kMaxKnown = 5000;

    void remember(const QUrl &url)
    {
        if (!m_known.contains(url)) {
            m_order.append(url);
            while (m_order.size() > kMaxKnown)
                m_known.remove(m_order.takeFirst());
        }
    }

    void lost()
    {
        if (m_retryPending)
            return;
        // Take the badges down but keep the items, so they come back on reconnecting.
        const QList<QUrl> urls = m_known.keys();
        for (const QUrl &u : urls) {
            if (!m_known.value(u).isEmpty()) {
                m_known.insert(u, QString());
                Q_EMIT overlaysChanged(u, {});
            }
        }
        if (m_retries >= kMaxRetries)
            return;
        m_retryPending = true;
        QTimer::singleShot(kRetryDelayMs, this, [this] {
            m_retryPending = false;
            ++m_retries;
            watch();
        });
    }

    void watch()
    {
        if (m_watch->state() == QLocalSocket::UnconnectedState)
            m_watch->connectToServer(socketPath());
    }

    // One short request: "Q <path>" lines and ".", one badge per line back.
    QStringList query(const QStringList &paths)
    {
        // The app isn't answering: don't slow the file manager down; the timer asks again.
        if (QDateTime::currentMSecsSinceEpoch() < m_quietUntil)
            return {};
        QLocalSocket s;
        s.connectToServer(socketPath());
        if (!s.waitForConnected(100)) {
            m_quietUntil = QDateTime::currentMSecsSinceEpoch() + kAskAgainMs;
            return {};
        }
        QByteArray req;
        for (const QString &p : paths) {
            if (p.contains(QLatin1Char('\n')))
                req += "Q \n";
            else
                req += "Q " + p.toUtf8() + "\n";
        }
        req += ".\n";
        s.write(req);
        s.waitForBytesWritten(300);
        QByteArray all;
        while (s.state() == QLocalSocket::ConnectedState && s.waitForReadyRead(500))
            all += s.readAll();
        all += s.readAll();
        if (all.isEmpty() && !paths.isEmpty()) {
            m_quietUntil = QDateTime::currentMSecsSinceEpoch() + kAskAgainMs;
            return {};
        }
        QStringList out = QString::fromUtf8(all).split(QLatin1Char('\n'));
        if (!out.isEmpty() && out.last().isEmpty())
            out.removeLast();
        return out;
    }

    // Statuses may have changed (or some weren't answered): ask again for every remembered item
    // and update what differs.
    void refresh()
    {
        QList<QUrl> urls = m_known.keys();
        QStringList paths;
        for (const QUrl &u : urls) {
            QString p = u.toLocalFile();
            while (p.size() > 1 && p.endsWith(QLatin1Char('/')))
                p.chop(1);
            paths << p;
        }
        if (paths.isEmpty())
            return;
        const QStringList answer = query(paths);
        if (answer.size() != urls.size()) {
            m_retryTimer->start(); // still no answer: try again
            return;
        }
        for (int i = 0; i < urls.size(); ++i) {
            if (m_known.value(urls[i]) != answer[i]) {
                m_known.insert(urls[i], answer[i]);
                Q_EMIT overlaysChanged(urls[i], iconsFor(answer[i]));
            }
        }
    }

    QLocalSocket *m_watch = nullptr;
    int m_retries = 0;
    bool m_retryPending = false;
    QHash<QUrl, QString> m_known;
    QList<QUrl> m_order;
    QTimer *m_retryTimer = nullptr;
    qint64 m_quietUntil = 0;
};

#include "drivelinux_overlay.moc"
