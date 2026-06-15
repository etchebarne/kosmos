#include "ipc/kosmos_ipc_client.h"

gboolean kosmos_ipc_client_activate_pane(
    KosmosIpcClient *self,
    guint64 workspace_id,
    guint64 pane_id,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_int_member(params, "paneId", (gint64)pane_id);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_PANE,
        KOSMOS_IPC_ACTION_ACTIVATE,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}

gboolean kosmos_ipc_client_split_pane(
    KosmosIpcClient *self,
    guint64 workspace_id,
    guint64 pane_id,
    KosmosIpcSplitAxis axis,
    gboolean new_pane_first,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_int_member(params, "paneId", (gint64)pane_id);
    json_object_set_string_member(params, "axis", kosmos_ipc_split_axis_to_string(axis));
    json_object_set_boolean_member(params, "newPaneFirst", new_pane_first);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_PANE,
        KOSMOS_IPC_ACTION_SPLIT,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}

gboolean kosmos_ipc_client_move_pane(
    KosmosIpcClient *self,
    guint64 workspace_id,
    guint64 pane_id,
    guint64 target_pane_id,
    KosmosIpcSplitAxis axis,
    gboolean new_pane_first,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_int_member(params, "paneId", (gint64)pane_id);
    json_object_set_int_member(params, "targetPaneId", (gint64)target_pane_id);
    json_object_set_string_member(params, "axis", kosmos_ipc_split_axis_to_string(axis));
    json_object_set_boolean_member(params, "newPaneFirst", new_pane_first);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_PANE,
        KOSMOS_IPC_ACTION_MOVE,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}
