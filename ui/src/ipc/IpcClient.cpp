#include "IpcClient.h"

#include <QJsonDocument>

IpcClient::IpcClient(QObject *parent) : QObject(parent) {
    connect(&socket, &QLocalSocket::connected, this, &IpcClient::connected);
    connect(&socket, &QLocalSocket::disconnected, this, &IpcClient::disconnected);
    connect(&socket, &QLocalSocket::readyRead, this, &IpcClient::readMessages);
    connect(&socket, &QLocalSocket::errorOccurred, this, [this](QLocalSocket::LocalSocketError) {
        emit errorOccurred(socket.errorString());
    });
}

void IpcClient::connectToServer(const QString &socketPath) {
    if (socket.state() != QLocalSocket::UnconnectedState) {
        socket.abort();
    }

    socket.connectToServer(socketPath);
}

quint64 IpcClient::sendRequest(const QString &domain, const QString &action, const QJsonObject &params) {
    const quint64 requestId = nextRequestId++;

    if (socket.state() != QLocalSocket::ConnectedState) {
        emit errorOccurred(QStringLiteral("IPC socket is not connected"));
        return requestId;
    }

    const QJsonObject message{
        {QStringLiteral("type"), QStringLiteral("request")},
        {QStringLiteral("id"), static_cast<qint64>(requestId)},
        {QStringLiteral("domain"), domain},
        {QStringLiteral("action"), action},
        {QStringLiteral("params"), params},
    };

    QByteArray payload = QJsonDocument(message).toJson(QJsonDocument::Compact);
    payload.append('\n');
    socket.write(payload);

    return requestId;
}

void IpcClient::readMessages() {
    buffer.append(socket.readAll());

    qsizetype newlineIndex = buffer.indexOf('\n');
    while (newlineIndex >= 0) {
        const QByteArray frame = buffer.left(newlineIndex).trimmed();
        buffer.remove(0, newlineIndex + 1);

        if (!frame.isEmpty()) {
            QJsonParseError parseError;
            const QJsonDocument document = QJsonDocument::fromJson(frame, &parseError);

            if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
                emit errorOccurred(QStringLiteral("Invalid IPC response: %1").arg(parseError.errorString()));
            } else {
                emit messageReceived(document.object());
            }
        }

        newlineIndex = buffer.indexOf('\n');
    }
}
