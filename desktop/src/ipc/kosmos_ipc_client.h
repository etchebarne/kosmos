#pragma once

#include <gio/gio.h>
#include <json-glib/json-glib.h>

#include "ipc/kosmos_ipc_protocol.h"

G_BEGIN_DECLS

#define KOSMOS_TYPE_IPC_CLIENT (kosmos_ipc_client_get_type())
#define KOSMOS_IPC_ERROR (kosmos_ipc_error_quark())

G_DECLARE_FINAL_TYPE(KosmosIpcClient, kosmos_ipc_client, KOSMOS, IPC_CLIENT, GObject)

typedef enum {
    KOSMOS_IPC_ERROR_CONNECTION,
    KOSMOS_IPC_ERROR_NOT_CONNECTED,
    KOSMOS_IPC_ERROR_IO,
    KOSMOS_IPC_ERROR_INVALID_RESPONSE,
    KOSMOS_IPC_ERROR_SERVER,
} KosmosIpcError;

GQuark kosmos_ipc_error_quark(void);

char *kosmos_ipc_default_socket_path(void);
KosmosIpcClient *kosmos_ipc_client_new(const char *socket_path);
KosmosIpcClient *kosmos_ipc_client_new_from_environment(void);

const char *kosmos_ipc_client_get_socket_path(KosmosIpcClient *self);
gboolean kosmos_ipc_client_is_connected(KosmosIpcClient *self);
gboolean kosmos_ipc_client_connect(KosmosIpcClient *self, GCancellable *cancellable, GError **error);
void kosmos_ipc_client_disconnect(KosmosIpcClient *self);

gboolean kosmos_ipc_client_request(
    KosmosIpcClient *self,
    KosmosIpcDomain domain,
    const char *action,
    JsonObject *params,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
);

gboolean kosmos_ipc_client_list_workspaces(
    KosmosIpcClient *self,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
);

gboolean kosmos_ipc_client_open_workspace(
    KosmosIpcClient *self,
    const char *path,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
);

gboolean kosmos_ipc_client_activate_workspace(
    KosmosIpcClient *self,
    guint64 workspace_id,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
);

G_END_DECLS
