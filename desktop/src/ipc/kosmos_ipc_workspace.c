#include "ipc/kosmos_ipc_client.h"

gboolean kosmos_ipc_client_list_workspaces(
    KosmosIpcClient *self,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    return kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_WORKSPACE,
        KOSMOS_IPC_ACTION_LIST,
        NULL,
        result,
        cancellable,
        error
    );
}

gboolean kosmos_ipc_client_open_workspace(
    KosmosIpcClient *self,
    const char *path,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    g_return_val_if_fail(path != NULL, FALSE);

    JsonObject *params = json_object_new();
    json_object_set_string_member(params, "path", path);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_WORKSPACE,
        KOSMOS_IPC_ACTION_OPEN,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}

gboolean kosmos_ipc_client_activate_workspace(
    KosmosIpcClient *self,
    guint64 workspace_id,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_WORKSPACE,
        KOSMOS_IPC_ACTION_ACTIVATE,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}
