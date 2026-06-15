#include "ipc/kosmos_ipc_client.h"

gboolean kosmos_ipc_client_list_file_tree(
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
        KOSMOS_IPC_DOMAIN_FILE_TREE,
        KOSMOS_IPC_ACTION_LIST,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}
