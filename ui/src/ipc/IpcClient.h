#pragma once

#include <QJsonObject>
#include <QLocalSocket>
#include <QObject>

class IpcClient final : public QObject {
    Q_OBJECT

public:
    explicit IpcClient(QObject *parent = nullptr);

    void connectToServer(const QString &socketPath);
    quint64 sendRequest(const QString &domain, const QString &action, const QJsonObject &params = {});

signals:
    void connected();
    void disconnected();
    void messageReceived(const QJsonObject &message);
    void errorOccurred(const QString &message);

private:
    void readMessages();

    QLocalSocket socket;
    QByteArray buffer;
    quint64 nextRequestId = 1;
};
