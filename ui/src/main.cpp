#include "ipc/IpcClient.h"

#include <QApplication>
#include <QDir>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLabel>
#include <QMainWindow>
#include <QPushButton>
#include <QTextEdit>
#include <QVBoxLayout>
#include <QWidget>

namespace {

QString socketPath() {
    const QString configuredPath = qEnvironmentVariable("KOSMOS_SOCKET");
    if (!configuredPath.isEmpty()) {
        return configuredPath;
    }

    QString runtimeDir = qEnvironmentVariable("XDG_RUNTIME_DIR");
    if (runtimeDir.isEmpty()) {
        runtimeDir = QDir::tempPath();
    }

    return QDir(runtimeDir).filePath(QStringLiteral("kosmos/server.sock"));
}

} // namespace

int main(int argc, char *argv[]) {
    QApplication app(argc, argv);

    QMainWindow window;
    window.setWindowTitle(QStringLiteral("Kosmos"));
    window.resize(1200, 800);

    auto *central = new QWidget(&window);
    auto *layout = new QVBoxLayout(central);
    auto *status = new QLabel(QStringLiteral("Connecting to server..."), central);
    auto *refresh = new QPushButton(QStringLiteral("Refresh workspaces"), central);
    auto *messages = new QTextEdit(central);

    status->setTextInteractionFlags(Qt::TextSelectableByMouse);
    messages->setReadOnly(true);

    layout->addWidget(status);
    layout->addWidget(refresh);
    layout->addWidget(messages, 1);
    window.setCentralWidget(central);

    IpcClient ipc;
    const QString serverSocketPath = socketPath();

    QObject::connect(&ipc, &IpcClient::connected, [&]() {
        status->setText(QStringLiteral("Connected to %1").arg(serverSocketPath));
        ipc.sendRequest(QStringLiteral("workspace"), QStringLiteral("list"));
    });
    QObject::connect(&ipc, &IpcClient::disconnected, [&]() {
        status->setText(QStringLiteral("Disconnected from server"));
    });
    QObject::connect(&ipc, &IpcClient::errorOccurred, [&](const QString &message) {
        status->setText(QStringLiteral("IPC error: %1").arg(message));
        messages->append(status->text());
    });
    QObject::connect(&ipc, &IpcClient::messageReceived, [&](const QJsonObject &message) {
        const QString json = QString::fromUtf8(QJsonDocument(message).toJson(QJsonDocument::Indented));
        messages->append(json);
    });
    QObject::connect(refresh, &QPushButton::clicked, [&]() {
        ipc.sendRequest(QStringLiteral("workspace"), QStringLiteral("list"));
    });

    ipc.connectToServer(serverSocketPath);

    window.show();

    return QApplication::exec();
}
