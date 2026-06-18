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

gboolean kosmos_ipc_client_create_file_tree_entry(
    KosmosIpcClient *self,
    guint64 workspace_id,
    const char *parent_path,
    const char *name,
    gboolean directory,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    g_return_val_if_fail(parent_path != NULL, FALSE);
    g_return_val_if_fail(name != NULL, FALSE);

    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_string_member(params, "parentPath", parent_path);
    json_object_set_string_member(params, "name", name);
    json_object_set_string_member(params, "kind", directory ? "directory" : "file");

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_FILE_TREE,
        KOSMOS_IPC_ACTION_CREATE,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}

gboolean kosmos_ipc_client_rename_file_tree_entry(
    KosmosIpcClient *self,
    guint64 workspace_id,
    const char *path,
    const char *name,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    g_return_val_if_fail(path != NULL, FALSE);
    g_return_val_if_fail(name != NULL, FALSE);

    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_string_member(params, "path", path);
    json_object_set_string_member(params, "name", name);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_FILE_TREE,
        KOSMOS_IPC_ACTION_RENAME,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}

gboolean kosmos_ipc_client_delete_file_tree_entry(
    KosmosIpcClient *self,
    guint64 workspace_id,
    const char *path,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    g_return_val_if_fail(path != NULL, FALSE);

    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_string_member(params, "path", path);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_FILE_TREE,
        KOSMOS_IPC_ACTION_DELETE,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}

gboolean kosmos_ipc_client_move_file_tree_entry(
    KosmosIpcClient *self,
    guint64 workspace_id,
    const char *path,
    const char *target_directory_path,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    g_return_val_if_fail(path != NULL, FALSE);
    g_return_val_if_fail(target_directory_path != NULL, FALSE);

    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_string_member(params, "path", path);
    json_object_set_string_member(params, "targetDirectoryPath", target_directory_path);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_FILE_TREE,
        KOSMOS_IPC_ACTION_MOVE,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}

gboolean kosmos_ipc_client_copy_file_tree_entry(
    KosmosIpcClient *self,
    guint64 workspace_id,
    const char *path,
    const char *target_directory_path,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    g_return_val_if_fail(path != NULL, FALSE);
    g_return_val_if_fail(target_directory_path != NULL, FALSE);

    JsonObject *params = json_object_new();
    json_object_set_int_member(params, "workspaceId", (gint64)workspace_id);
    json_object_set_string_member(params, "path", path);
    json_object_set_string_member(params, "targetDirectoryPath", target_directory_path);

    gboolean requested = kosmos_ipc_client_request(
        self,
        KOSMOS_IPC_DOMAIN_FILE_TREE,
        KOSMOS_IPC_ACTION_COPY,
        params,
        result,
        cancellable,
        error
    );

    json_object_unref(params);
    return requested;
}
