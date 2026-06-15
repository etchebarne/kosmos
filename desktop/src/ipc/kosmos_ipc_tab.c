#include "ipc/kosmos_ipc_client.h"

gboolean kosmos_ipc_client_open_tab(
    KosmosIpcClient *self,
    guint64 workspace_id,
    guint64 pane_id,
    const char *title,
    KosmosIpcTabKind kind,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_int_member(params, "paneId", (gint64)pane_id);
    json_object_set_string_member(params, "title", title == NULL ? "Blank" : title);
    json_object_set_string_member(params, "kind", kosmos_ipc_tab_kind_to_string(kind));

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_TAB,
        KOSMOS_IPC_ACTION_OPEN,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}

gboolean kosmos_ipc_client_activate_tab(
    KosmosIpcClient *self,
    guint64 workspace_id,
    guint64 pane_id,
    guint64 tab_id,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_int_member(params, "paneId", (gint64)pane_id);
    json_object_set_int_member(params, "tabId", (gint64)tab_id);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_TAB,
        KOSMOS_IPC_ACTION_ACTIVATE,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}

gboolean kosmos_ipc_client_close_tab(
    KosmosIpcClient *self,
    guint64 workspace_id,
    guint64 pane_id,
    guint64 tab_id,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_int_member(params, "paneId", (gint64)pane_id);
    json_object_set_int_member(params, "tabId", (gint64)tab_id);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_TAB,
        KOSMOS_IPC_ACTION_CLOSE,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}

gboolean kosmos_ipc_client_set_tab_kind(
    KosmosIpcClient *self,
    guint64 workspace_id,
    guint64 pane_id,
    guint64 tab_id,
    KosmosIpcTabKind kind,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_int_member(params, "paneId", (gint64)pane_id);
    json_object_set_int_member(params, "tabId", (gint64)tab_id);
    json_object_set_string_member(params, "kind", kosmos_ipc_tab_kind_to_string(kind));

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_TAB,
        KOSMOS_IPC_ACTION_SET_KIND,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}

gboolean kosmos_ipc_client_reorder_tab(
    KosmosIpcClient *self,
    guint64 workspace_id,
    guint64 pane_id,
    guint64 tab_id,
    guint target_index,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_int_member(params, "paneId", (gint64)pane_id);
    json_object_set_int_member(params, "tabId", (gint64)tab_id);
    json_object_set_int_member(params, "targetIndex", (gint64)target_index);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_TAB,
        KOSMOS_IPC_ACTION_REORDER,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}

gboolean kosmos_ipc_client_split_tab(
    KosmosIpcClient *self,
    guint64 workspace_id,
    guint64 pane_id,
    guint64 target_pane_id,
    guint64 tab_id,
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
    json_object_set_int_member(params, "tabId", (gint64)tab_id);
    json_object_set_string_member(params, "axis", kosmos_ipc_split_axis_to_string(axis));
    json_object_set_boolean_member(params, "newPaneFirst", new_pane_first);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_TAB,
        KOSMOS_IPC_ACTION_SPLIT,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}
