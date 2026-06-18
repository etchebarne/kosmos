#include "tabs/kosmos_file_tree_tab.h"

#include <adwaita.h>

#define KOSMOS_TYPE_FILE_TREE_ITEM (kosmos_file_tree_item_get_type())
#define KOSMOS_FILE_TREE_ITEM(object) (G_TYPE_CHECK_INSTANCE_CAST((object), KOSMOS_TYPE_FILE_TREE_ITEM, KosmosFileTreeItem))
#define FILE_TREE_MONITOR_MAX_DIRECTORIES 256
#define FILE_TREE_MONITOR_MAX_DEPTH 4

typedef struct _KosmosFileTreeItem KosmosFileTreeItem;
typedef struct _KosmosFileTreeItemClass KosmosFileTreeItemClass;

typedef struct {
    char *path;
    char *kind;
    char *name;
} FileTreeEntryRef;

typedef struct {
    KosmosIpcClient *ipc_client;
    guint64 workspace_id;
    GtkWidget *root;
    GtkWidget *notice_label;
    GtkWidget *scrolled;
    GtkWidget *list;
    GtkTreeListModel *tree_model;
    GHashTable *expanded_paths;
    GPtrArray *monitors;
    guint monitor_refresh_source_id;
    guint busy_count;
    char *root_path;
    char *root_name;
    char *focused_path;
    char *pending_focus_path;
    char *editing_path;
    GPtrArray *clipboard_entries;
    gboolean clipboard_cut;
} FileTreeContext;

typedef enum {
    FILE_TREE_ASYNC_LIST,
    FILE_TREE_ASYNC_DELETE,
    FILE_TREE_ASYNC_MOVE,
    FILE_TREE_ASYNC_COPY,
} FileTreeAsyncOperation;

typedef struct {
    GtkWidget *root;
    KosmosIpcClient *ipc_client;
    guint64 workspace_id;
    FileTreeAsyncOperation operation;
    char *path;
    char *target_directory_path;
    GPtrArray *entries;
    char *pending_focus_path;
    char *error_prefix;
    gboolean clear_clipboard_on_success;
    gboolean show_progress;
} FileTreeAsyncRequest;

typedef enum {
    FILE_TREE_DIALOG_CREATE_FILE,
    FILE_TREE_DIALOG_CREATE_DIRECTORY,
} FileTreeDialogOperation;

typedef struct {
    FileTreeContext *context;
    FileTreeDialogOperation operation;
    char *path;
    AdwDialog *dialog;
    GtkWidget *entry;
    GtkWidget *error_label;
} FileTreeNameDialog;

typedef struct {
    FileTreeContext *context;
    GtkWidget *root;
    GPtrArray *entries;
} FileTreeDeleteDialog;

typedef enum {
    FILE_TREE_MENU_NEW_FILE,
    FILE_TREE_MENU_NEW_FOLDER,
    FILE_TREE_MENU_RENAME,
    FILE_TREE_MENU_DELETE,
    FILE_TREE_MENU_REFRESH,
    FILE_TREE_MENU_CUT,
    FILE_TREE_MENU_COPY,
    FILE_TREE_MENU_PASTE,
    FILE_TREE_MENU_REVEAL,
} FileTreeMenuOperation;

typedef struct {
    FileTreeContext *context;
    FileTreeMenuOperation operation;
    char *path;
    char *kind;
    char *name;
    GPtrArray *entries;
    GtkWidget *popover;
} FileTreeMenuAction;

struct _KosmosFileTreeItem {
    GObject parent_instance;
    char *name;
    char *path;
    char *kind;
    GListStore *children;
};

struct _KosmosFileTreeItemClass {
    GObjectClass parent_class;
};

GType kosmos_file_tree_item_get_type(void);
G_DEFINE_FINAL_TYPE(KosmosFileTreeItem, kosmos_file_tree_item, G_TYPE_OBJECT)

static void kosmos_file_tree_item_finalize(GObject *object) {
    KosmosFileTreeItem *self = KOSMOS_FILE_TREE_ITEM(object);

    g_clear_pointer(&self->name, g_free);
    g_clear_pointer(&self->path, g_free);
    g_clear_pointer(&self->kind, g_free);
    g_clear_object(&self->children);

    G_OBJECT_CLASS(kosmos_file_tree_item_parent_class)->finalize(object);
}

static void kosmos_file_tree_item_class_init(KosmosFileTreeItemClass *klass) {
    GObjectClass *object_class = G_OBJECT_CLASS(klass);
    object_class->finalize = kosmos_file_tree_item_finalize;
}

static void kosmos_file_tree_item_init(KosmosFileTreeItem *self) {
    self->children = g_list_store_new(KOSMOS_TYPE_FILE_TREE_ITEM);
}

static FileTreeEntryRef *file_tree_entry_ref_new(const char *path, const char *kind, const char *name) {
    FileTreeEntryRef *entry = g_new0(FileTreeEntryRef, 1);

    entry->path = g_strdup(path);
    entry->kind = g_strdup(kind);
    entry->name = g_strdup(name);

    return entry;
}

static FileTreeEntryRef *file_tree_entry_ref_new_from_item(KosmosFileTreeItem *item) {
    return file_tree_entry_ref_new(item->path, item->kind, item->name);
}

static void file_tree_entry_ref_free(FileTreeEntryRef *entry) {
    g_clear_pointer(&entry->path, g_free);
    g_clear_pointer(&entry->kind, g_free);
    g_clear_pointer(&entry->name, g_free);
    g_free(entry);
}

static void file_tree_context_free(FileTreeContext *context) {
    if (context->monitor_refresh_source_id != 0) {
        g_source_remove(context->monitor_refresh_source_id);
    }

    g_clear_object(&context->ipc_client);
    g_clear_object(&context->tree_model);
    g_clear_pointer(&context->expanded_paths, g_hash_table_unref);
    g_clear_pointer(&context->monitors, g_ptr_array_unref);
    g_clear_pointer(&context->root_path, g_free);
    g_clear_pointer(&context->root_name, g_free);
    g_clear_pointer(&context->focused_path, g_free);
    g_clear_pointer(&context->pending_focus_path, g_free);
    g_clear_pointer(&context->editing_path, g_free);
    g_clear_pointer(&context->clipboard_entries, g_ptr_array_unref);
    g_free(context);
}

static void file_tree_async_request_free(FileTreeAsyncRequest *request) {
    g_clear_object(&request->root);
    g_clear_object(&request->ipc_client);
    g_clear_pointer(&request->path, g_free);
    g_clear_pointer(&request->target_directory_path, g_free);
    g_clear_pointer(&request->entries, g_ptr_array_unref);
    g_clear_pointer(&request->pending_focus_path, g_free);
    g_clear_pointer(&request->error_prefix, g_free);
    g_free(request);
}

static void file_tree_name_dialog_free(FileTreeNameDialog *request) {
    g_clear_pointer(&request->path, g_free);
    g_free(request);
}

static void file_tree_delete_dialog_free(FileTreeDeleteDialog *request) {
    g_clear_object(&request->root);
    g_clear_pointer(&request->entries, g_ptr_array_unref);
    g_free(request);
}

static void file_tree_menu_action_free(FileTreeMenuAction *action) {
    g_clear_pointer(&action->path, g_free);
    g_clear_pointer(&action->kind, g_free);
    g_clear_pointer(&action->name, g_free);
    g_clear_pointer(&action->entries, g_ptr_array_unref);
    g_clear_object(&action->popover);
    g_free(action);
}

static KosmosFileTreeItem *kosmos_file_tree_item_new(const char *name, const char *path, const char *kind) {
    KosmosFileTreeItem *item = g_object_new(KOSMOS_TYPE_FILE_TREE_ITEM, NULL);

    item->name = g_strdup(name == NULL || name[0] == '\0' ? "Untitled" : name);
    item->path = g_strdup(path == NULL ? "" : path);
    item->kind = g_strdup(kind == NULL || kind[0] == '\0' ? "other" : kind);

    return item;
}

static KosmosFileTreeItem *kosmos_file_tree_item_new_message(const char *message) {
    return kosmos_file_tree_item_new(message, "", "message");
}

static gboolean kosmos_file_tree_item_is_directory(KosmosFileTreeItem *item) {
    return g_strcmp0(item->kind, "directory") == 0;
}

static gboolean kosmos_file_tree_item_is_message(KosmosFileTreeItem *item) {
    return g_strcmp0(item->kind, "message") == 0;
}

static guint kosmos_file_tree_item_child_count(KosmosFileTreeItem *item) {
    return g_list_model_get_n_items(G_LIST_MODEL(item->children));
}

static gboolean get_string_member(JsonObject *object, const char *name, const char **value) {
    JsonNode *node = json_object_get_member(object, name);
    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node) || json_node_get_value_type(node) != G_TYPE_STRING) {
        return FALSE;
    }

    *value = json_node_get_string(node);
    return TRUE;
}

static gboolean get_bool_member(JsonObject *object, const char *name, gboolean *value) {
    JsonNode *node = json_object_get_member(object, name);
    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node) || json_node_get_value_type(node) != G_TYPE_BOOLEAN) {
        return FALSE;
    }

    *value = json_node_get_boolean(node);
    return TRUE;
}

static JsonObject *get_object_member(JsonObject *object, const char *name) {
    JsonNode *node = json_object_get_member(object, name);

    if (node == NULL || !JSON_NODE_HOLDS_OBJECT(node)) {
        return NULL;
    }

    return json_node_get_object(node);
}

static JsonArray *get_array_member(JsonObject *object, const char *name) {
    JsonNode *node = json_object_get_member(object, name);

    if (node == NULL || !JSON_NODE_HOLDS_ARRAY(node)) {
        return NULL;
    }

    return json_node_get_array(node);
}

static GtkWidget *create_status_label(const char *text, const char *css_class) {
    GtkWidget *content = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    GtkWidget *label = gtk_label_new(text);

    gtk_label_set_wrap(GTK_LABEL(label), TRUE);
    gtk_label_set_xalign(GTK_LABEL(label), 0.5f);
    gtk_widget_set_halign(label, GTK_ALIGN_CENTER);
    gtk_widget_set_valign(label, GTK_ALIGN_CENTER);
    gtk_widget_set_hexpand(label, TRUE);
    gtk_widget_set_vexpand(label, TRUE);
    gtk_widget_set_hexpand(content, TRUE);
    gtk_widget_set_vexpand(content, TRUE);

    if (css_class != NULL) {
        gtk_widget_add_css_class(label, css_class);
    }

    gtk_box_append(GTK_BOX(content), label);
    return content;
}

static GtkWidget *create_file_tree_list(FileTreeContext *context, JsonObject *root);
static gboolean file_tree_item_is_root(FileTreeContext *context, KosmosFileTreeItem *item);
static gboolean path_is_equal_or_descendant(const char *path, const char *parent);
static void file_tree_context_apply_tree(FileTreeContext *context, JsonNode *tree);
static void apply_file_tree_operation_response(FileTreeContext *context, JsonNode *tree, GError *error, const char *prefix);
static void file_tree_context_refresh(FileTreeContext *context);
static void file_tree_context_refresh_with_progress(FileTreeContext *context, gboolean show_progress);
static GdkContentProvider *file_tree_drag_prepare(GtkDragSource *source, double x, double y, gpointer user_data);
static GdkDragAction file_tree_drop_enter(GtkDropTarget *target, double x, double y, gpointer user_data);
static GdkDragAction file_tree_drop_motion(GtkDropTarget *target, double x, double y, gpointer user_data);
static void file_tree_drop_leave(GtkDropTarget *target, gpointer user_data);
static gboolean file_tree_drop(GtkDropTarget *target, const GValue *value, double x, double y, gpointer user_data);
static void inline_rename_commit(GtkWidget *entry);
static void inline_rename_cancel(GtkWidget *entry);
static gboolean inline_rename_key_pressed(GtkEventControllerKey *controller, guint keyval, guint keycode, GdkModifierType state, gpointer user_data);
static void inline_rename_focus_leave(GtkEventControllerFocus *controller, gpointer user_data);

static void file_tree_install_css(GtkWidget *widget) {
    static gboolean installed = FALSE;

    if (installed) {
        return;
    }

    GtkCssProvider *provider = gtk_css_provider_new();
    GdkDisplay *display = gtk_widget_get_display(widget);
    if (display == NULL) {
        display = gdk_display_get_default();
    }

    if (display == NULL) {
        g_object_unref(provider);
        return;
    }

    gtk_css_provider_load_from_string(
        provider,
        ".file-tree-drop-target {"
        "  background: rgba(46, 160, 67, 0.24);"
        "}"
    );
    gtk_style_context_add_provider_for_display(
        display,
        GTK_STYLE_PROVIDER(provider),
        GTK_STYLE_PROVIDER_PRIORITY_APPLICATION
    );
    g_object_unref(provider);
    installed = TRUE;
}

static void file_tree_context_set_focused_path(FileTreeContext *context, const char *path) {
    if (g_strcmp0(context->focused_path, path) == 0) {
        return;
    }

    g_free(context->focused_path);
    context->focused_path = g_strdup(path);
}

static void file_tree_context_set_pending_focus_path(FileTreeContext *context, const char *path) {
    g_free(context->pending_focus_path);
    context->pending_focus_path = g_strdup(path);
}

static void file_tree_context_set_editing_path(FileTreeContext *context, const char *path) {
    g_free(context->editing_path);
    context->editing_path = g_strdup(path);
}

static void file_tree_context_clear_editing_path(FileTreeContext *context) {
    g_clear_pointer(&context->editing_path, g_free);
}

static char *file_tree_path_child(const char *parent, const char *name) {
    return g_build_filename(parent, name, NULL);
}

static char *file_tree_numbered_copy_name(const char *name, guint index) {
    const char *dot = strrchr(name, '.');

    if (dot != NULL && dot > name && dot[1] != '\0') {
        return g_strdup_printf("%.*s (%u)%s", (int)(dot - name), name, index, dot);
    }

    return g_strdup_printf("%s (%u)", name, index);
}

static char *file_tree_available_copy_path(const char *target_directory, const char *name) {
    char *target = file_tree_path_child(target_directory, name);

    if (!g_file_test(target, G_FILE_TEST_EXISTS | G_FILE_TEST_IS_SYMLINK)) {
        return target;
    }

    g_free(target);

    for (guint index = 1;; index++) {
        char *numbered_name = file_tree_numbered_copy_name(name, index);
        target = file_tree_path_child(target_directory, numbered_name);
        g_free(numbered_name);

        if (!g_file_test(target, G_FILE_TEST_EXISTS | G_FILE_TEST_IS_SYMLINK)) {
            return target;
        }

        g_free(target);
    }
}

static gboolean file_tree_name_is_valid(const char *name) {
    return name != NULL && name[0] != '\0' && g_strcmp0(name, ".") != 0 && g_strcmp0(name, "..") != 0 && strchr(name, G_DIR_SEPARATOR) == NULL && strchr(name, '/') == NULL;
}

static char *file_tree_path_parent(const char *path) {
    return g_path_get_dirname(path);
}

static char *file_tree_paste_target_directory(const char *path, const char *kind) {
    return g_strcmp0(kind, "directory") == 0 ? g_strdup(path) : file_tree_path_parent(path);
}

static GPtrArray *file_tree_entry_ref_array_new(void) {
    return g_ptr_array_new_with_free_func((GDestroyNotify)file_tree_entry_ref_free);
}

static GPtrArray *file_tree_entry_ref_array_copy(GPtrArray *entries) {
    GPtrArray *copy = file_tree_entry_ref_array_new();

    for (guint index = 0; index < entries->len; index++) {
        FileTreeEntryRef *entry = g_ptr_array_index(entries, index);
        g_ptr_array_add(copy, file_tree_entry_ref_new(entry->path, entry->kind, entry->name));
    }

    return copy;
}

static void file_tree_context_store_clipboard(FileTreeContext *context, GPtrArray *entries, gboolean cut) {
    g_clear_pointer(&context->clipboard_entries, g_ptr_array_unref);
    context->clipboard_entries = file_tree_entry_ref_array_new();

    for (guint index = 0; index < entries->len; index++) {
        FileTreeEntryRef *entry = g_ptr_array_index(entries, index);
        g_ptr_array_add(context->clipboard_entries, file_tree_entry_ref_new(entry->path, entry->kind, entry->name));
    }

    context->clipboard_cut = cut;
}

static void file_tree_context_clear_clipboard(FileTreeContext *context) {
    g_clear_pointer(&context->clipboard_entries, g_ptr_array_unref);
    context->clipboard_cut = FALSE;
}

static gboolean file_tree_context_has_clipboard(FileTreeContext *context) {
    if (context->clipboard_entries == NULL || context->clipboard_entries->len == 0) {
        return FALSE;
    }

    for (guint index = 0; index < context->clipboard_entries->len; index++) {
        FileTreeEntryRef *entry = g_ptr_array_index(context->clipboard_entries, index);
        if (!g_file_test(entry->path, G_FILE_TEST_EXISTS | G_FILE_TEST_IS_SYMLINK)) {
            file_tree_context_clear_clipboard(context);
            return FALSE;
        }
    }

    return TRUE;
}

static gboolean file_tree_context_clipboard_intersects_entries(FileTreeContext *context, GPtrArray *entries) {
    if (context->clipboard_entries == NULL) {
        return FALSE;
    }

    for (guint clipboard_index = 0; clipboard_index < context->clipboard_entries->len; clipboard_index++) {
        FileTreeEntryRef *clipboard_entry = g_ptr_array_index(context->clipboard_entries, clipboard_index);
        for (guint entry_index = 0; entry_index < entries->len; entry_index++) {
            FileTreeEntryRef *entry = g_ptr_array_index(entries, entry_index);
            if (path_is_equal_or_descendant(clipboard_entry->path, entry->path)) {
                return TRUE;
            }
        }
    }

    return FALSE;
}

static const char *file_tree_context_clipboard_conflict_name(FileTreeContext *context, const char *target_directory) {
    if (context->clipboard_entries == NULL) {
        return NULL;
    }

    for (guint index = 0; index < context->clipboard_entries->len; index++) {
        FileTreeEntryRef *entry = g_ptr_array_index(context->clipboard_entries, index);
        char *target = file_tree_path_child(target_directory, entry->name);
        gboolean conflict = g_file_test(target, G_FILE_TEST_EXISTS | G_FILE_TEST_IS_SYMLINK);
        g_free(target);

        if (conflict) {
            return entry->name;
        }
    }

    return NULL;
}

static void file_tree_context_set_notice(FileTreeContext *context, const char *message, const char *css_class) {
    if (context->notice_label == NULL) {
        return;
    }

    gtk_label_set_text(GTK_LABEL(context->notice_label), message == NULL ? "" : message);
    gtk_widget_set_visible(context->notice_label, message != NULL && message[0] != '\0');
    gtk_widget_remove_css_class(context->notice_label, "error");
    gtk_widget_remove_css_class(context->notice_label, "dim-label");

    if (css_class != NULL) {
        gtk_widget_add_css_class(context->notice_label, css_class);
    }
}

static void file_tree_context_begin_operation(FileTreeContext *context, const char *message) {
    context->busy_count++;
    file_tree_context_set_notice(context, message == NULL ? "Working..." : message, "dim-label");
}

static void file_tree_context_finish_operation(FileTreeContext *context) {
    if (context->busy_count > 0) {
        context->busy_count--;
    }

    if (context->busy_count == 0) {
        file_tree_context_set_notice(context, NULL, NULL);
    }
}

static FileTreeAsyncRequest *file_tree_async_request_new(
    FileTreeContext *context,
    FileTreeAsyncOperation operation,
    const char *error_prefix
) {
    FileTreeAsyncRequest *request = g_new0(FileTreeAsyncRequest, 1);

    request->root = g_object_ref(context->root);
    request->ipc_client = g_object_ref(context->ipc_client);
    request->workspace_id = context->workspace_id;
    request->operation = operation;
    request->error_prefix = g_strdup(error_prefix);

    return request;
}

static void file_tree_async_request_thread(GTask *task, gpointer source_object, gpointer task_data, GCancellable *cancellable) {
    (void)source_object;

    FileTreeAsyncRequest *request = task_data;
    GError *error = NULL;
    JsonNode *tree = NULL;
    gboolean ok = FALSE;

    if (request->entries != NULL) {
        for (guint index = 0; index < request->entries->len; index++) {
            FileTreeEntryRef *entry = g_ptr_array_index(request->entries, index);

            g_clear_pointer(&tree, json_node_unref);
            switch (request->operation) {
            case FILE_TREE_ASYNC_DELETE:
                ok = kosmos_ipc_client_delete_file_tree_entry(request->ipc_client, request->workspace_id, entry->path, &tree, cancellable, &error);
                break;
            case FILE_TREE_ASYNC_MOVE:
                ok = kosmos_ipc_client_move_file_tree_entry(request->ipc_client, request->workspace_id, entry->path, request->target_directory_path, &tree, cancellable, &error);
                break;
            case FILE_TREE_ASYNC_COPY:
                ok = kosmos_ipc_client_copy_file_tree_entry(request->ipc_client, request->workspace_id, entry->path, request->target_directory_path, &tree, cancellable, &error);
                break;
            case FILE_TREE_ASYNC_LIST:
                ok = kosmos_ipc_client_list_file_tree(request->ipc_client, request->workspace_id, &tree, cancellable, &error);
                break;
            }

            if (!ok || error != NULL) {
                if (tree != NULL) {
                    json_node_unref(tree);
                }
                g_task_return_error(task, error == NULL ? g_error_new(KOSMOS_IPC_ERROR, KOSMOS_IPC_ERROR_IO, "IPC request failed") : error);
                return;
            }
        }

        g_task_return_pointer(task, tree, (GDestroyNotify)json_node_unref);
        return;
    }

    switch (request->operation) {
    case FILE_TREE_ASYNC_LIST:
        ok = kosmos_ipc_client_list_file_tree(request->ipc_client, request->workspace_id, &tree, cancellable, &error);
        break;
    case FILE_TREE_ASYNC_DELETE:
        ok = kosmos_ipc_client_delete_file_tree_entry(request->ipc_client, request->workspace_id, request->path, &tree, cancellable, &error);
        break;
    case FILE_TREE_ASYNC_MOVE:
        ok = kosmos_ipc_client_move_file_tree_entry(request->ipc_client, request->workspace_id, request->path, request->target_directory_path, &tree, cancellable, &error);
        break;
    case FILE_TREE_ASYNC_COPY:
        ok = kosmos_ipc_client_copy_file_tree_entry(request->ipc_client, request->workspace_id, request->path, request->target_directory_path, &tree, cancellable, &error);
        break;
    }

    if (!ok || error != NULL) {
        if (tree != NULL) {
            json_node_unref(tree);
        }
        g_task_return_error(task, error == NULL ? g_error_new(KOSMOS_IPC_ERROR, KOSMOS_IPC_ERROR_IO, "IPC request failed") : error);
        return;
    }

    g_task_return_pointer(task, tree, (GDestroyNotify)json_node_unref);
}

static void file_tree_async_request_done(GObject *source_object, GAsyncResult *result, gpointer user_data) {
    (void)source_object;
    (void)user_data;

    GTask *task = G_TASK(result);
    FileTreeAsyncRequest *request = g_task_get_task_data(task);
    FileTreeContext *context = g_object_get_data(G_OBJECT(request->root), "file-tree-context");
    GError *error = NULL;
    JsonNode *tree = g_task_propagate_pointer(task, &error);

    if (context == NULL) {
        g_clear_error(&error);
        if (tree != NULL) {
            json_node_unref(tree);
        }
        return;
    }

    if (request->show_progress) {
        file_tree_context_finish_operation(context);
    }

    if (error != NULL) {
        char *message = g_strdup_printf("%s: %s", request->error_prefix, error->message);
        file_tree_context_set_notice(context, message, "error");
        g_free(message);
        g_clear_error(&error);
        file_tree_context_refresh_with_progress(context, FALSE);
        return;
    }

    if (request->pending_focus_path != NULL) {
        file_tree_context_set_pending_focus_path(context, request->pending_focus_path);
    }
    if (request->clear_clipboard_on_success) {
        file_tree_context_clear_clipboard(context);
    }

    file_tree_context_apply_tree(context, tree);
    if (tree != NULL) {
        json_node_unref(tree);
    }
}

static void file_tree_async_request_start(FileTreeContext *context, FileTreeAsyncRequest *request, const char *progress_message) {
    if (request->show_progress) {
        file_tree_context_begin_operation(context, progress_message);
    }

    GTask *task = g_task_new(request->ipc_client, NULL, file_tree_async_request_done, NULL);
    g_task_set_task_data(task, request, (GDestroyNotify)file_tree_async_request_free);
    g_task_run_in_thread(task, file_tree_async_request_thread);
    g_object_unref(task);
}

static void file_tree_copy_conflict_response(AdwAlertDialog *dialog, GAsyncResult *result, gpointer user_data) {
    FileTreeAsyncRequest *request = user_data;
    const char *response = adw_alert_dialog_choose_finish(dialog, result);
    FileTreeContext *context = g_object_get_data(G_OBJECT(request->root), "file-tree-context");

    if (context != NULL && g_strcmp0(response, "keep-both") == 0) {
        gboolean batch = request->entries != NULL && request->entries->len > 1;
        file_tree_async_request_start(context, request, batch ? "Copying entries..." : "Copying entry...");
        return;
    }

    file_tree_async_request_free(request);
}

static void file_tree_confirm_copy_conflict(FileTreeContext *context, FileTreeAsyncRequest *request, const char *name) {
    AdwDialog *dialog = adw_alert_dialog_new("Name Already Exists", NULL);
    adw_alert_dialog_format_body(
        ADW_ALERT_DIALOG(dialog),
        "An entry named \"%s\" already exists in this folder. Keep both and create a numbered copy?",
        name
    );
    adw_alert_dialog_add_responses(ADW_ALERT_DIALOG(dialog), "cancel", "_Cancel", "keep-both", "Keep _Both", NULL);
    adw_alert_dialog_set_close_response(ADW_ALERT_DIALOG(dialog), "cancel");
    adw_alert_dialog_set_default_response(ADW_ALERT_DIALOG(dialog), "keep-both");
    adw_alert_dialog_choose(ADW_ALERT_DIALOG(dialog), context->root, NULL, (GAsyncReadyCallback)file_tree_copy_conflict_response, request);
}

static void file_tree_monitor_free(gpointer data) {
    GFileMonitor *monitor = data;

    g_file_monitor_cancel(monitor);
    g_object_unref(monitor);
}

static gboolean file_tree_monitor_refresh(gpointer user_data);

static void file_tree_context_schedule_monitor_refresh(FileTreeContext *context) {
    if (context->monitor_refresh_source_id != 0) {
        return;
    }

    context->monitor_refresh_source_id = g_timeout_add(150, file_tree_monitor_refresh, context);
}

static gboolean file_tree_monitor_refresh(gpointer user_data) {
    FileTreeContext *context = user_data;

    context->monitor_refresh_source_id = 0;
    file_tree_context_refresh_with_progress(context, FALSE);

    return G_SOURCE_REMOVE;
}

static void file_tree_monitor_changed(
    GFileMonitor *monitor,
    GFile *file,
    GFile *other_file,
    GFileMonitorEvent event,
    gpointer user_data
) {
    (void)monitor;
    (void)file;
    (void)other_file;
    (void)event;

    file_tree_context_schedule_monitor_refresh(user_data);
}

static void file_tree_context_watch_directory(FileTreeContext *context, const char *path) {
    if (context->monitors->len >= FILE_TREE_MONITOR_MAX_DIRECTORIES) {
        return;
    }

    GFile *file = g_file_new_for_path(path);
    GError *error = NULL;
    GFileMonitor *monitor = g_file_monitor_directory(file, G_FILE_MONITOR_NONE, NULL, &error);

    if (monitor != NULL && context->monitors->len < FILE_TREE_MONITOR_MAX_DIRECTORIES) {
        g_signal_connect(monitor, "changed", G_CALLBACK(file_tree_monitor_changed), context);
        g_ptr_array_add(context->monitors, monitor);
        monitor = NULL;
    }

    g_clear_object(&monitor);
    g_clear_error(&error);
    g_object_unref(file);
}

static void file_tree_context_watch_item(FileTreeContext *context, KosmosFileTreeItem *item, GHashTable *paths, guint depth) {
    if (depth > FILE_TREE_MONITOR_MAX_DEPTH || context->monitors->len >= FILE_TREE_MONITOR_MAX_DIRECTORIES) {
        return;
    }

    if (kosmos_file_tree_item_is_directory(item) && item->path[0] != '\0' && g_hash_table_add(paths, g_strdup(item->path))) {
        file_tree_context_watch_directory(context, item->path);
    }

    if (depth == FILE_TREE_MONITOR_MAX_DEPTH) {
        return;
    }

    guint child_count = kosmos_file_tree_item_child_count(item);
    for (guint index = 0; index < child_count; index++) {
        KosmosFileTreeItem *child = g_list_model_get_item(G_LIST_MODEL(item->children), index);

        if (child != NULL && !kosmos_file_tree_item_is_message(child)) {
            file_tree_context_watch_item(context, child, paths, depth + 1);
        }

        g_clear_object(&child);
    }
}

static void file_tree_context_rebuild_monitors(FileTreeContext *context, KosmosFileTreeItem *root) {
    g_clear_pointer(&context->monitors, g_ptr_array_unref);
    context->monitors = g_ptr_array_new_with_free_func(file_tree_monitor_free);

    GHashTable *paths = g_hash_table_new_full(g_str_hash, g_str_equal, g_free, NULL);
    file_tree_context_watch_item(context, root, paths, 0);
    g_hash_table_unref(paths);
}

static void file_tree_context_remember_expanded(FileTreeContext *context) {
    if (context->tree_model == NULL) {
        return;
    }

    GHashTable *expanded_paths = g_hash_table_new_full(g_str_hash, g_str_equal, g_free, NULL);
    guint row_count = g_list_model_get_n_items(G_LIST_MODEL(context->tree_model));

    for (guint index = 0; index < row_count; index++) {
        GtkTreeListRow *row = g_list_model_get_item(G_LIST_MODEL(context->tree_model), index);
        if (row == NULL) {
            continue;
        }

        KosmosFileTreeItem *item = KOSMOS_FILE_TREE_ITEM(gtk_tree_list_row_get_item(row));
        if (kosmos_file_tree_item_is_directory(item) && !file_tree_item_is_root(context, item) &&
            gtk_tree_list_row_get_expanded(row)) {
            g_hash_table_add(expanded_paths, g_strdup(item->path));
        }

        g_object_unref(row);
    }

    g_clear_pointer(&context->expanded_paths, g_hash_table_unref);
    context->expanded_paths = expanded_paths;
}

static gboolean file_tree_context_path_was_expanded(FileTreeContext *context, const char *path) {
    return context->expanded_paths != NULL && g_hash_table_contains(context->expanded_paths, path);
}

static void file_tree_context_restore_expanded(FileTreeContext *context, GtkTreeListModel *tree_model) {
    for (guint index = 0; index < g_list_model_get_n_items(G_LIST_MODEL(tree_model)); index++) {
        GtkTreeListRow *row = g_list_model_get_item(G_LIST_MODEL(tree_model), index);
        if (row == NULL) {
            continue;
        }

        KosmosFileTreeItem *item = KOSMOS_FILE_TREE_ITEM(gtk_tree_list_row_get_item(row));
        if (kosmos_file_tree_item_is_directory(item) &&
            (file_tree_item_is_root(context, item) || file_tree_context_path_was_expanded(context, item->path))) {
            gtk_tree_list_row_set_expanded(row, TRUE);
        }

        g_object_unref(row);
    }
}

static void file_tree_context_set_status(FileTreeContext *context, const char *text, const char *css_class) {
    context->list = NULL;
    g_clear_object(&context->tree_model);
    g_clear_pointer(&context->monitors, g_ptr_array_unref);
    gtk_scrolled_window_set_child(GTK_SCROLLED_WINDOW(context->scrolled), create_status_label(text, css_class));
}

static void file_tree_context_apply_tree(FileTreeContext *context, JsonNode *tree) {
    if (tree == NULL || !JSON_NODE_HOLDS_OBJECT(tree)) {
        file_tree_context_set_status(context, "File tree is unavailable.", "dim-label");
        return;
    }

    JsonObject *result = json_node_get_object(tree);
    JsonObject *root = get_object_member(result, "root");
    const char *root_path = NULL;
    const char *root_name = NULL;
    if (root == NULL || !get_string_member(root, "path", &root_path) || !get_string_member(root, "name", &root_name)) {
        file_tree_context_set_status(context, "File tree response is invalid.", "error");
        return;
    }

    file_tree_context_remember_expanded(context);
    g_clear_object(&context->tree_model);
    if (context->pending_focus_path != NULL) {
        file_tree_context_set_focused_path(context, context->pending_focus_path);
        g_clear_pointer(&context->pending_focus_path, g_free);
    } else if (context->focused_path == NULL) {
        file_tree_context_set_focused_path(context, root_path);
    }

    g_free(context->root_path);
    g_free(context->root_name);
    context->root_path = g_strdup(root_path);
    context->root_name = g_strdup(root_name);
    context->list = create_file_tree_list(context, root);
    gtk_scrolled_window_set_child(GTK_SCROLLED_WINDOW(context->scrolled), context->list);
    gtk_widget_grab_focus(context->list);
}

static void file_tree_context_apply_response(FileTreeContext *context, JsonNode *tree, GError *error, const char *prefix) {
    if (error != NULL) {
        char *message = g_strdup_printf("%s: %s", prefix, error->message);
        file_tree_context_set_status(context, message, "error");
        g_free(message);
        return;
    }

    file_tree_context_apply_tree(context, tree);
}

static void file_tree_context_refresh_with_progress(FileTreeContext *context, gboolean show_progress) {
    if (context->monitor_refresh_source_id != 0) {
        g_source_remove(context->monitor_refresh_source_id);
        context->monitor_refresh_source_id = 0;
    }

    FileTreeAsyncRequest *request = file_tree_async_request_new(context, FILE_TREE_ASYNC_LIST, "Failed to load file tree");
    request->show_progress = show_progress;
    file_tree_async_request_start(context, request, "Refreshing file tree...");
}

static void file_tree_context_refresh(FileTreeContext *context) {
    file_tree_context_refresh_with_progress(context, TRUE);
}

static const char *entry_icon_name(KosmosFileTreeItem *item) {
    if (kosmos_file_tree_item_is_directory(item)) {
        return "folder-symbolic";
    }
    if (kosmos_file_tree_item_is_message(item)) {
        return "dialog-information-symbolic";
    }
    if (g_strcmp0(item->kind, "symlink") == 0) {
        return "emblem-symbolic-link-symbolic";
    }

    return "text-x-generic-symbolic";
}

static void append_message_child(KosmosFileTreeItem *item, const char *message) {
    KosmosFileTreeItem *message_item = kosmos_file_tree_item_new_message(message);
    g_list_store_append(item->children, message_item);
    g_object_unref(message_item);
}

static KosmosFileTreeItem *file_tree_item_from_entry(JsonObject *entry) {
    const char *name = NULL;
    const char *path = NULL;
    const char *kind = NULL;
    const char *read_error = NULL;
    gboolean children_truncated = FALSE;

    if (!get_string_member(entry, "name", &name) || !get_string_member(entry, "kind", &kind)) {
        return kosmos_file_tree_item_new_message("Invalid file tree entry.");
    }

    get_string_member(entry, "path", &path);
    get_string_member(entry, "readError", &read_error);
    get_bool_member(entry, "childrenTruncated", &children_truncated);

    KosmosFileTreeItem *item = kosmos_file_tree_item_new(name, path, kind);

    JsonArray *children = get_array_member(entry, "children");
    guint child_count = children == NULL ? 0 : json_array_get_length(children);
    for (guint index = 0; index < child_count; index++) {
        JsonNode *child_node = json_array_get_element(children, index);
        if (child_node == NULL || !JSON_NODE_HOLDS_OBJECT(child_node)) {
            append_message_child(item, "Invalid file tree entry.");
            continue;
        }

        KosmosFileTreeItem *child = file_tree_item_from_entry(json_node_get_object(child_node));
        g_list_store_append(item->children, child);
        g_object_unref(child);
    }

    if (children_truncated) {
        append_message_child(item, "Directory listing is truncated.");
    }
    if (read_error != NULL) {
        append_message_child(item, read_error);
    }

    return item;
}

static GListModel *create_child_model(gpointer item, gpointer user_data) {
    (void)user_data;

    KosmosFileTreeItem *entry = KOSMOS_FILE_TREE_ITEM(item);
    if (kosmos_file_tree_item_child_count(entry) == 0) {
        return NULL;
    }

    return G_LIST_MODEL(g_object_ref(entry->children));
}

static void setup_list_item(GtkSignalListItemFactory *factory, GtkListItem *list_item, gpointer user_data) {
    (void)factory;
    (void)user_data;

    GtkWidget *hit_row = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
    GtkWidget *row = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 6);
    GtkWidget *icon = gtk_image_new();
    GtkWidget *label = gtk_label_new(NULL);
    GtkWidget *entry = gtk_entry_new();
    GtkDragSource *drag_source = gtk_drag_source_new();
    GtkDropTarget *drop_target = gtk_drop_target_new(KOSMOS_TYPE_FILE_TREE_ITEM, GDK_ACTION_MOVE);
    GtkEventController *entry_key = gtk_event_controller_key_new();
    GtkEventController *entry_focus = gtk_event_controller_focus_new();

    gtk_list_item_set_selectable(list_item, TRUE);
    gtk_widget_set_margin_top(row, 3);
    gtk_widget_set_margin_bottom(row, 3);
    gtk_widget_set_margin_end(row, 10);
    gtk_widget_set_hexpand(hit_row, TRUE);
    gtk_widget_set_halign(hit_row, GTK_ALIGN_FILL);
    gtk_widget_set_hexpand(row, TRUE);
    gtk_widget_set_halign(row, GTK_ALIGN_FILL);
    gtk_label_set_xalign(GTK_LABEL(label), 0.0f);
    gtk_label_set_ellipsize(GTK_LABEL(label), PANGO_ELLIPSIZE_END);
    gtk_widget_set_hexpand(label, TRUE);
    gtk_widget_set_hexpand(entry, TRUE);
    gtk_widget_set_visible(entry, FALSE);

    gtk_box_append(GTK_BOX(row), icon);
    gtk_box_append(GTK_BOX(row), label);
    gtk_box_append(GTK_BOX(row), entry);
    gtk_box_append(GTK_BOX(hit_row), row);
    g_object_set_data(G_OBJECT(hit_row), "entry-row", row);
    g_object_set_data(G_OBJECT(hit_row), "entry-icon", icon);
    g_object_set_data(G_OBJECT(hit_row), "entry-label", label);
    g_object_set_data(G_OBJECT(hit_row), "entry-editor", entry);
    gtk_drag_source_set_actions(drag_source, GDK_ACTION_MOVE);
    gtk_drop_target_set_preload(drop_target, TRUE);
    g_signal_connect(drag_source, "prepare", G_CALLBACK(file_tree_drag_prepare), NULL);
    g_signal_connect(drop_target, "enter", G_CALLBACK(file_tree_drop_enter), NULL);
    g_signal_connect(drop_target, "motion", G_CALLBACK(file_tree_drop_motion), NULL);
    g_signal_connect(drop_target, "leave", G_CALLBACK(file_tree_drop_leave), NULL);
    g_signal_connect(drop_target, "drop", G_CALLBACK(file_tree_drop), NULL);
    gtk_widget_add_controller(hit_row, GTK_EVENT_CONTROLLER(drag_source));
    gtk_widget_add_controller(hit_row, GTK_EVENT_CONTROLLER(drop_target));
    g_signal_connect(entry, "activate", G_CALLBACK(inline_rename_commit), NULL);
    g_signal_connect(entry_key, "key-pressed", G_CALLBACK(inline_rename_key_pressed), NULL);
    g_signal_connect(entry_focus, "leave", G_CALLBACK(inline_rename_focus_leave), NULL);
    gtk_widget_add_controller(entry, entry_key);
    gtk_widget_add_controller(entry, entry_focus);

    gtk_list_item_set_child(list_item, hit_row);
}

static void set_tree_row_data(GtkWidget *widget, GtkTreeListRow *tree_row) {
    g_object_set_data_full(G_OBJECT(widget), "tree-row", g_object_ref(tree_row), g_object_unref);
}

static GtkTreeListRow *tree_row_data(GtkWidget *widget) {
    GtkTreeListRow *tree_row = g_object_get_data(G_OBJECT(widget), "tree-row");

    return tree_row == NULL ? NULL : g_object_ref(tree_row);
}

static void bind_list_item(GtkSignalListItemFactory *factory, GtkListItem *list_item, gpointer user_data) {
    (void)factory;

    FileTreeContext *context = user_data;
    GtkTreeListRow *tree_row = gtk_list_item_get_item(list_item);
    KosmosFileTreeItem *item = KOSMOS_FILE_TREE_ITEM(gtk_tree_list_row_get_item(tree_row));
    GtkWidget *hit_row = gtk_list_item_get_child(list_item);
    GtkWidget *row = g_object_get_data(G_OBJECT(hit_row), "entry-row");
    GtkWidget *icon = g_object_get_data(G_OBJECT(hit_row), "entry-icon");
    GtkWidget *label = g_object_get_data(G_OBJECT(hit_row), "entry-label");
    GtkWidget *entry = g_object_get_data(G_OBJECT(hit_row), "entry-editor");
    guint depth = gtk_tree_list_row_get_depth(tree_row);
    gboolean editing = g_strcmp0(context->editing_path, item->path) == 0;

    gtk_widget_set_margin_start(row, 10 + (int)(depth * 18));
    gtk_image_set_from_icon_name(GTK_IMAGE(icon), entry_icon_name(item));
    gtk_label_set_text(GTK_LABEL(label), item->name);
    gtk_editable_set_text(GTK_EDITABLE(entry), item->name);
    gtk_widget_set_visible(label, !editing);
    gtk_widget_set_visible(entry, editing);
    gtk_widget_set_tooltip_text(hit_row, item->path[0] == '\0' ? NULL : item->path);
    set_tree_row_data(hit_row, tree_row);
    g_object_set_data(G_OBJECT(hit_row), "file-tree-context", context);
    g_object_set_data_full(G_OBJECT(hit_row), "file-tree-item", g_object_ref(item), g_object_unref);
    g_object_set_data(G_OBJECT(entry), "file-tree-context", context);
    g_object_set_data_full(G_OBJECT(entry), "file-tree-item", g_object_ref(item), g_object_unref);

    if (kosmos_file_tree_item_is_message(item)) {
        gtk_widget_add_css_class(label, "dim-label");
        gtk_widget_add_css_class(icon, "dim-label");
    } else {
        gtk_widget_remove_css_class(label, "dim-label");
        gtk_widget_remove_css_class(icon, "dim-label");
    }

    if (editing) {
        gtk_widget_grab_focus(entry);
        gtk_editable_select_region(GTK_EDITABLE(entry), 0, -1);
    }
}

static GtkTreeListRow *tree_row_in_descendants(GtkWidget *widget) {
    if (widget == NULL) {
        return NULL;
    }

    GtkTreeListRow *tree_row = tree_row_data(widget);
    if (tree_row != NULL) {
        return tree_row;
    }

    for (GtkWidget *child = gtk_widget_get_first_child(widget); child != NULL; child = gtk_widget_get_next_sibling(child)) {
        GtkTreeListRow *child_row = tree_row_in_descendants(child);
        if (child_row != NULL) {
            return child_row;
        }
    }

    return NULL;
}

static GtkTreeListRow *tree_row_in_ancestors(GtkWidget *widget) {
    for (GtkWidget *ancestor = widget; ancestor != NULL; ancestor = gtk_widget_get_parent(ancestor)) {
        GtkTreeListRow *tree_row = tree_row_data(ancestor);
        if (tree_row != NULL) {
            return tree_row;
        }
    }

    return NULL;
}

static GtkTreeListRow *tree_row_for_picked_widget(GtkWidget *widget) {
    gboolean picked_list_item = FALSE;

    for (GtkWidget *ancestor = widget; ancestor != NULL; ancestor = gtk_widget_get_parent(ancestor)) {
        if (g_strcmp0(G_OBJECT_TYPE_NAME(ancestor), "GtkListItemWidget") == 0) {
            picked_list_item = TRUE;
            break;
        }
    }

    if (!picked_list_item) {
        return NULL;
    }

    GtkTreeListRow *tree_row = tree_row_in_ancestors(widget);

    return tree_row == NULL ? tree_row_in_descendants(widget) : tree_row;
}

static gboolean file_tree_model_index_for_path(GtkTreeListModel *tree_model, const char *path, guint *position) {
    if (tree_model == NULL || path == NULL) {
        return FALSE;
    }

    guint row_count = g_list_model_get_n_items(G_LIST_MODEL(tree_model));
    for (guint index = 0; index < row_count; index++) {
        GtkTreeListRow *row = g_list_model_get_item(G_LIST_MODEL(tree_model), index);
        if (row == NULL) {
            continue;
        }

        KosmosFileTreeItem *item = KOSMOS_FILE_TREE_ITEM(gtk_tree_list_row_get_item(row));
        gboolean matches = g_strcmp0(item->path, path) == 0;
        g_object_unref(row);

        if (matches) {
            *position = index;
            return TRUE;
        }
    }

    return FALSE;
}

static void file_tree_context_select_focused_path(FileTreeContext *context) {
    if (context->list == NULL || context->focused_path == NULL) {
        return;
    }

    GtkSelectionModel *selection = gtk_list_view_get_model(GTK_LIST_VIEW(context->list));
    guint position = 0;
    if (file_tree_model_index_for_path(context->tree_model, context->focused_path, &position)) {
        gtk_selection_model_select_item(selection, position, TRUE);
    }
}

static void file_tree_clicked(GtkGestureClick *gesture, int n_press, double x, double y, gpointer user_data) {
    (void)n_press;

    FileTreeContext *context = user_data;
    GdkModifierType state = gtk_event_controller_get_current_event_state(GTK_EVENT_CONTROLLER(gesture));
    gboolean multi_select_click =
        (state & (GDK_SHIFT_MASK | GDK_CONTROL_MASK)) != 0;

    if (context->list == NULL) {
        return;
    }

    GtkWidget *source = gtk_event_controller_get_widget(GTK_EVENT_CONTROLLER(gesture));
    graphene_point_t source_point = GRAPHENE_POINT_INIT((float)x, (float)y);
    graphene_point_t list_point = GRAPHENE_POINT_INIT(0.0f, 0.0f);
    GtkTreeListRow *tree_row = NULL;

    if (gtk_widget_compute_point(source, context->list, &source_point, &list_point)) {
        GtkWidget *picked = gtk_widget_pick(context->list, list_point.x, list_point.y, GTK_PICK_DEFAULT);
        tree_row = tree_row_for_picked_widget(picked);
    }

    if (tree_row != NULL) {
        KosmosFileTreeItem *item = KOSMOS_FILE_TREE_ITEM(gtk_tree_list_row_get_item(tree_row));
        if (!kosmos_file_tree_item_is_message(item)) {
            file_tree_context_set_focused_path(context, item->path);
        }
        if (!multi_select_click && gtk_tree_list_row_is_expandable(tree_row)) {
            gtk_tree_list_row_set_expanded(tree_row, !gtk_tree_list_row_get_expanded(tree_row));
        }
    }

    g_clear_object(&tree_row);
}

static GtkWindow *file_tree_context_window(FileTreeContext *context) {
    GtkRoot *root = gtk_widget_get_root(context->root);

    return GTK_IS_WINDOW(root) ? GTK_WINDOW(root) : NULL;
}

static gboolean file_tree_item_is_root(FileTreeContext *context, KosmosFileTreeItem *item) {
    return context->root_path != NULL && g_strcmp0(context->root_path, item->path) == 0;
}

static void apply_file_tree_operation_response(FileTreeContext *context, JsonNode *tree, GError *error, const char *prefix) {
    file_tree_context_apply_response(context, tree, error, prefix);
    g_clear_error(&error);
    if (tree != NULL) {
        json_node_unref(tree);
    }
}

static gboolean path_is_equal_or_descendant(const char *path, const char *parent) {
    gsize parent_len = strlen(parent);

    return g_strcmp0(path, parent) == 0 ||
        (g_str_has_prefix(path, parent) && G_IS_DIR_SEPARATOR(path[parent_len]));
}

static gboolean file_tree_entry_refs_contain_ancestor(GPtrArray *entries, const char *path) {
    for (guint index = 0; index < entries->len; index++) {
        FileTreeEntryRef *entry = g_ptr_array_index(entries, index);
        if (path_is_equal_or_descendant(path, entry->path)) {
            return TRUE;
        }
    }

    return FALSE;
}

static void file_tree_entry_refs_remove_descendants(GPtrArray *entries, const char *path) {
    for (guint index = entries->len; index > 0; index--) {
        FileTreeEntryRef *entry = g_ptr_array_index(entries, index - 1);
        if (path_is_equal_or_descendant(entry->path, path)) {
            g_ptr_array_remove_index(entries, index - 1);
        }
    }
}

static void file_tree_entry_refs_add_filtered(GPtrArray *entries, KosmosFileTreeItem *item) {
    if (file_tree_entry_refs_contain_ancestor(entries, item->path)) {
        return;
    }

    file_tree_entry_refs_remove_descendants(entries, item->path);
    g_ptr_array_add(entries, file_tree_entry_ref_new_from_item(item));
}

static GPtrArray *file_tree_context_selected_entries(FileTreeContext *context, gboolean include_root) {
    GPtrArray *entries = file_tree_entry_ref_array_new();

    if (context->list == NULL || context->tree_model == NULL) {
        return entries;
    }

    GtkSelectionModel *selection = gtk_list_view_get_model(GTK_LIST_VIEW(context->list));
    guint row_count = g_list_model_get_n_items(G_LIST_MODEL(context->tree_model));
    for (guint index = 0; index < row_count; index++) {
        if (!gtk_selection_model_is_selected(selection, index)) {
            continue;
        }

        GtkTreeListRow *row = g_list_model_get_item(G_LIST_MODEL(context->tree_model), index);
        if (row == NULL) {
            continue;
        }

        KosmosFileTreeItem *item = KOSMOS_FILE_TREE_ITEM(gtk_tree_list_row_get_item(row));
        if (!kosmos_file_tree_item_is_message(item) && (include_root || !file_tree_item_is_root(context, item))) {
            file_tree_entry_refs_add_filtered(entries, item);
        }

        g_object_unref(row);
    }

    return entries;
}

static gboolean file_tree_context_path_is_selected(FileTreeContext *context, const char *path) {
    guint position = 0;

    if (context->list == NULL || !file_tree_model_index_for_path(context->tree_model, path, &position)) {
        return FALSE;
    }

    GtkSelectionModel *selection = gtk_list_view_get_model(GTK_LIST_VIEW(context->list));
    return gtk_selection_model_is_selected(selection, position);
}

static GPtrArray *file_tree_context_action_entries(FileTreeContext *context, KosmosFileTreeItem *item) {
    if (!file_tree_item_is_root(context, item) && file_tree_context_path_is_selected(context, item->path)) {
        GPtrArray *selected = file_tree_context_selected_entries(context, FALSE);
        if (selected->len > 0) {
            return selected;
        }

        g_ptr_array_unref(selected);
    }

    GPtrArray *entries = file_tree_entry_ref_array_new();
    if (!file_tree_item_is_root(context, item)) {
        g_ptr_array_add(entries, file_tree_entry_ref_new_from_item(item));
    }
    return entries;
}

static gboolean file_tree_item_can_drag(FileTreeContext *context, KosmosFileTreeItem *item) {
    return item != NULL && !kosmos_file_tree_item_is_message(item) && !file_tree_item_is_root(context, item);
}

static gboolean file_tree_item_can_drop_on(
    FileTreeContext *context,
    KosmosFileTreeItem *source,
    KosmosFileTreeItem *target
) {
    if (!file_tree_item_can_drag(context, source) || target == NULL || !kosmos_file_tree_item_is_directory(target)) {
        return FALSE;
    }

    char *source_parent = g_path_get_dirname(source->path);
    gboolean same_parent = g_strcmp0(source_parent, target->path) == 0;
    g_free(source_parent);

    if (same_parent || g_strcmp0(source->path, target->path) == 0) {
        return FALSE;
    }

    return !kosmos_file_tree_item_is_directory(source) || !path_is_equal_or_descendant(target->path, source->path);
}

static GdkContentProvider *file_tree_drag_prepare(GtkDragSource *source, double x, double y, gpointer user_data) {
    (void)x;
    (void)y;
    (void)user_data;

    GtkWidget *row = gtk_event_controller_get_widget(GTK_EVENT_CONTROLLER(source));
    FileTreeContext *context = g_object_get_data(G_OBJECT(row), "file-tree-context");
    KosmosFileTreeItem *item = g_object_get_data(G_OBJECT(row), "file-tree-item");

    if (context == NULL || !file_tree_item_can_drag(context, item)) {
        return NULL;
    }

    return gdk_content_provider_new_typed(KOSMOS_TYPE_FILE_TREE_ITEM, item);
}

static KosmosFileTreeItem *file_tree_drop_source_item(GtkDropTarget *target) {
    const GValue *value = gtk_drop_target_get_value(target);

    return value == NULL ? NULL : g_value_get_object(value);
}

static GdkDragAction file_tree_drop_highlight(GtkDropTarget *target) {
    GtkWidget *row = gtk_event_controller_get_widget(GTK_EVENT_CONTROLLER(target));
    FileTreeContext *context = g_object_get_data(G_OBJECT(row), "file-tree-context");
    KosmosFileTreeItem *source = file_tree_drop_source_item(target);
    KosmosFileTreeItem *target_item = g_object_get_data(G_OBJECT(row), "file-tree-item");
    gboolean valid = context != NULL && file_tree_item_can_drop_on(context, source, target_item);

    if (valid) {
        gtk_widget_add_css_class(row, "file-tree-drop-target");
        return GDK_ACTION_MOVE;
    }

    gtk_widget_remove_css_class(row, "file-tree-drop-target");
    return 0;
}

static GdkDragAction file_tree_drop_enter(GtkDropTarget *target, double x, double y, gpointer user_data) {
    (void)x;
    (void)y;
    (void)user_data;

    return file_tree_drop_highlight(target);
}

static GdkDragAction file_tree_drop_motion(GtkDropTarget *target, double x, double y, gpointer user_data) {
    (void)x;
    (void)y;
    (void)user_data;

    return file_tree_drop_highlight(target);
}

static void file_tree_drop_leave(GtkDropTarget *target, gpointer user_data) {
    (void)user_data;

    GtkWidget *row = gtk_event_controller_get_widget(GTK_EVENT_CONTROLLER(target));
    gtk_widget_remove_css_class(row, "file-tree-drop-target");
}

static gboolean file_tree_drop(GtkDropTarget *target, const GValue *value, double x, double y, gpointer user_data) {
    (void)x;
    (void)y;
    (void)user_data;

    GtkWidget *row = gtk_event_controller_get_widget(GTK_EVENT_CONTROLLER(target));
    FileTreeContext *context = g_object_get_data(G_OBJECT(row), "file-tree-context");
    KosmosFileTreeItem *source = g_value_get_object(value);
    KosmosFileTreeItem *target_item = g_object_get_data(G_OBJECT(row), "file-tree-item");
    gtk_widget_remove_css_class(row, "file-tree-drop-target");

    if (context == NULL || !file_tree_item_can_drop_on(context, source, target_item)) {
        return FALSE;
    }

    char *focused_path = file_tree_path_child(target_item->path, source->name);
    FileTreeAsyncRequest *request = file_tree_async_request_new(context, FILE_TREE_ASYNC_MOVE, "Failed to move entry");
    request->path = g_strdup(source->path);
    request->target_directory_path = g_strdup(target_item->path);
    request->pending_focus_path = g_strdup(focused_path);
    request->show_progress = TRUE;
    file_tree_async_request_start(context, request, "Moving entry...");
    g_free(focused_path);
    return TRUE;
}

static void inline_rename_commit(GtkWidget *entry) {
    FileTreeContext *context = g_object_get_data(G_OBJECT(entry), "file-tree-context");
    KosmosFileTreeItem *item = g_object_get_data(G_OBJECT(entry), "file-tree-item");

    if (context == NULL || item == NULL || g_strcmp0(context->editing_path, item->path) != 0) {
        return;
    }

    const char *name = gtk_editable_get_text(GTK_EDITABLE(entry));
    if (name == NULL || name[0] == '\0' || g_strcmp0(name, item->name) == 0) {
        inline_rename_cancel(entry);
        return;
    }

    char *parent = file_tree_path_parent(item->path);
    char *focused_path = file_tree_path_child(parent, name);
    file_tree_context_set_pending_focus_path(context, focused_path);
    file_tree_context_clear_editing_path(context);

    GError *error = NULL;
    JsonNode *tree = NULL;
    kosmos_ipc_client_rename_file_tree_entry(
        context->ipc_client,
        context->workspace_id,
        item->path,
        name,
        &tree,
        NULL,
        &error
    );
    apply_file_tree_operation_response(context, tree, error, "Failed to rename entry");

    g_free(focused_path);
    g_free(parent);
}

static void inline_rename_cancel(GtkWidget *entry) {
    FileTreeContext *context = g_object_get_data(G_OBJECT(entry), "file-tree-context");
    KosmosFileTreeItem *item = g_object_get_data(G_OBJECT(entry), "file-tree-item");

    if (context == NULL || item == NULL || g_strcmp0(context->editing_path, item->path) != 0) {
        return;
    }

    file_tree_context_clear_editing_path(context);
    file_tree_context_refresh(context);
}

static gboolean inline_rename_key_pressed(
    GtkEventControllerKey *controller,
    guint keyval,
    guint keycode,
    GdkModifierType state,
    gpointer user_data
) {
    (void)keycode;
    (void)state;
    (void)user_data;

    if (keyval == GDK_KEY_Escape) {
        inline_rename_cancel(gtk_event_controller_get_widget(GTK_EVENT_CONTROLLER(controller)));
        return TRUE;
    }

    return FALSE;
}

static void inline_rename_focus_leave(GtkEventControllerFocus *controller, gpointer user_data) {
    (void)user_data;

    inline_rename_commit(gtk_event_controller_get_widget(GTK_EVENT_CONTROLLER(controller)));
}

static void file_tree_name_dialog_set_error(FileTreeNameDialog *request, const char *message) {
    gtk_label_set_text(GTK_LABEL(request->error_label), message);
    gtk_widget_set_visible(request->error_label, TRUE);
    gtk_widget_add_css_class(request->entry, "error");
    gtk_widget_grab_focus(request->entry);
}

static gboolean file_tree_name_dialog_validate(FileTreeNameDialog *request, const char *name) {
    if (!file_tree_name_is_valid(name)) {
        file_tree_name_dialog_set_error(request, "Enter a valid name.");
        return FALSE;
    }

    char *target = file_tree_path_child(request->path, name);
    gboolean exists = g_file_test(target, G_FILE_TEST_EXISTS | G_FILE_TEST_IS_SYMLINK);
    g_free(target);

    if (exists) {
        file_tree_name_dialog_set_error(request, "An entry with that name already exists.");
        return FALSE;
    }

    gtk_widget_remove_css_class(request->entry, "error");
    gtk_widget_set_visible(request->error_label, FALSE);
    return TRUE;
}

static void file_tree_name_dialog_entry_changed(GtkEditable *editable, gpointer user_data) {
    FileTreeNameDialog *request = user_data;

    gtk_widget_remove_css_class(GTK_WIDGET(editable), "error");
    gtk_widget_set_visible(request->error_label, FALSE);
}

static void file_tree_name_dialog_accept(GtkWidget *widget, gpointer user_data) {
    (void)widget;

    FileTreeNameDialog *request = user_data;
    gboolean created = FALSE;

    const char *name = gtk_editable_get_text(GTK_EDITABLE(request->entry));
    GError *error = NULL;
    JsonNode *tree = NULL;

    if (!file_tree_name_dialog_validate(request, name)) {
        return;
    }

    switch (request->operation) {
    case FILE_TREE_DIALOG_CREATE_FILE:
        {
            char *focused_path = file_tree_path_child(request->path, name);
            file_tree_context_set_pending_focus_path(request->context, focused_path);
            g_free(focused_path);
        }
        created = kosmos_ipc_client_create_file_tree_entry(
            request->context->ipc_client,
            request->context->workspace_id,
            request->path,
            name,
            FALSE,
            &tree,
            NULL,
            &error
        );
        break;
    case FILE_TREE_DIALOG_CREATE_DIRECTORY:
        {
            char *focused_path = file_tree_path_child(request->path, name);
            file_tree_context_set_pending_focus_path(request->context, focused_path);
            g_free(focused_path);
        }
        created = kosmos_ipc_client_create_file_tree_entry(
            request->context->ipc_client,
            request->context->workspace_id,
            request->path,
            name,
            TRUE,
            &tree,
            NULL,
            &error
        );
        break;
    }

    if (!created || error != NULL) {
        const char *message = error == NULL ? "Failed to create entry." : error->message;
        g_clear_pointer(&request->context->pending_focus_path, g_free);
        file_tree_name_dialog_set_error(request, message);
        g_clear_error(&error);
        if (tree != NULL) {
            json_node_unref(tree);
        }
        return;
    }

    file_tree_context_apply_tree(request->context, tree);
    if (tree != NULL) {
        json_node_unref(tree);
    }

    adw_dialog_close(request->dialog);
}

static void file_tree_name_dialog_cancel(GtkWidget *widget, gpointer user_data) {
    (void)widget;

    FileTreeNameDialog *request = user_data;
    adw_dialog_close(request->dialog);
}

static void show_name_dialog(
    FileTreeContext *context,
    FileTreeDialogOperation operation,
    const char *path,
    const char *initial_name
) {
    const char *title = "Name";
    const char *accept = "_Save";

    if (operation == FILE_TREE_DIALOG_CREATE_FILE) {
        title = "New File";
        accept = "_Create";
    } else if (operation == FILE_TREE_DIALOG_CREATE_DIRECTORY) {
        title = "New Folder";
        accept = "_Create";
    }

    AdwDialog *dialog = adw_dialog_new();
    GtkWidget *content = gtk_box_new(GTK_ORIENTATION_VERTICAL, 12);
    GtkWidget *entry = gtk_entry_new();
    GtkWidget *error_label = gtk_label_new(NULL);
    GtkWidget *buttons = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 6);
    GtkWidget *cancel_button = gtk_button_new_with_mnemonic("_Cancel");
    GtkWidget *accept_button = gtk_button_new_with_mnemonic(accept);
    FileTreeNameDialog *request = g_new0(FileTreeNameDialog, 1);

    request->context = context;
    request->operation = operation;
    request->path = g_strdup(path);
    request->dialog = dialog;
    request->entry = entry;
    request->error_label = error_label;

    adw_dialog_set_title(dialog, title);
    gtk_widget_set_margin_top(content, 18);
    gtk_widget_set_margin_bottom(content, 18);
    gtk_widget_set_margin_start(content, 18);
    gtk_widget_set_margin_end(content, 18);
    gtk_editable_set_text(GTK_EDITABLE(entry), initial_name == NULL ? "" : initial_name);
    gtk_widget_set_hexpand(entry, TRUE);
    gtk_label_set_xalign(GTK_LABEL(error_label), 0.0f);
    gtk_label_set_wrap(GTK_LABEL(error_label), TRUE);
    gtk_widget_add_css_class(error_label, "error");
    gtk_widget_set_visible(error_label, FALSE);
    gtk_widget_set_hexpand(buttons, TRUE);
    gtk_widget_set_halign(buttons, GTK_ALIGN_END);
    gtk_widget_add_css_class(accept_button, "suggested-action");

    gtk_box_append(GTK_BOX(content), entry);
    gtk_box_append(GTK_BOX(content), error_label);
    gtk_box_append(GTK_BOX(buttons), cancel_button);
    gtk_box_append(GTK_BOX(buttons), accept_button);
    gtk_box_append(GTK_BOX(content), buttons);
    adw_dialog_set_child(dialog, content);
    g_object_set_data_full(G_OBJECT(dialog), "file-tree-name-request", request, (GDestroyNotify)file_tree_name_dialog_free);
    g_signal_connect(entry, "changed", G_CALLBACK(file_tree_name_dialog_entry_changed), request);
    g_signal_connect(entry, "activate", G_CALLBACK(file_tree_name_dialog_accept), request);
    g_signal_connect(cancel_button, "clicked", G_CALLBACK(file_tree_name_dialog_cancel), request);
    g_signal_connect(accept_button, "clicked", G_CALLBACK(file_tree_name_dialog_accept), request);
    adw_dialog_present(dialog, context->root);
    gtk_widget_grab_focus(entry);
}

static void file_tree_delete_dialog_response(AdwAlertDialog *dialog, GAsyncResult *result, gpointer user_data) {
    (void)dialog;

    FileTreeDeleteDialog *request = user_data;
    FileTreeContext *context = g_object_get_data(G_OBJECT(request->root), "file-tree-context");
    const char *response = adw_alert_dialog_choose_finish(dialog, result);

    if (context != NULL && g_strcmp0(response, "delete") == 0) {
        FileTreeEntryRef *first_entry = g_ptr_array_index(request->entries, 0);
        char *focused_path = file_tree_path_parent(first_entry->path);
        FileTreeAsyncRequest *async_request = file_tree_async_request_new(context, FILE_TREE_ASYNC_DELETE, "Failed to delete entry");

        async_request->entries = file_tree_entry_ref_array_copy(request->entries);
        async_request->pending_focus_path = g_strdup(focused_path);
        async_request->show_progress = TRUE;
        async_request->clear_clipboard_on_success = file_tree_context_clipboard_intersects_entries(context, request->entries);
        file_tree_async_request_start(context, async_request, request->entries->len == 1 ? "Deleting entry..." : "Deleting entries...");
        g_free(focused_path);
    }

    file_tree_delete_dialog_free(request);
}

static void show_delete_dialog(FileTreeContext *context, GPtrArray *entries) {
    if (entries->len == 0) {
        return;
    }

    FileTreeEntryRef *first_entry = g_ptr_array_index(entries, 0);
    const char *type = g_strcmp0(first_entry->kind, "directory") == 0 ? "folder" : "file";
    char *body = entries->len == 1
        ? g_strdup_printf("%s\n\nThis cannot be undone.", first_entry->path)
        : g_strdup_printf("Delete %u selected entries?\n\nThis cannot be undone.", entries->len);
    AdwDialog *dialog = adw_alert_dialog_new(NULL, body);
    FileTreeDeleteDialog *request = g_new0(FileTreeDeleteDialog, 1);

    if (entries->len == 1) {
        adw_alert_dialog_format_heading(ADW_ALERT_DIALOG(dialog), "Delete %s \"%s\"?", type, first_entry->name);
    } else {
        adw_alert_dialog_set_heading(ADW_ALERT_DIALOG(dialog), "Delete Selected Entries?");
    }
    adw_alert_dialog_add_responses(ADW_ALERT_DIALOG(dialog), "cancel", "_Cancel", "delete", "_Delete", NULL);
    adw_alert_dialog_set_close_response(ADW_ALERT_DIALOG(dialog), "cancel");
    adw_alert_dialog_set_default_response(ADW_ALERT_DIALOG(dialog), "cancel");
    adw_alert_dialog_set_response_appearance(ADW_ALERT_DIALOG(dialog), "delete", ADW_RESPONSE_DESTRUCTIVE);

    g_free(body);
    request->context = context;
    request->root = g_object_ref(context->root);
    request->entries = file_tree_entry_ref_array_copy(entries);

    adw_alert_dialog_choose(ADW_ALERT_DIALOG(dialog), context->root, NULL, (GAsyncReadyCallback)file_tree_delete_dialog_response, request);
}

static void reveal_file_tree_entry(FileTreeContext *context, KosmosFileTreeItem *item) {
    GFile *file = g_file_new_for_path(item->path);
    GFile *target = kosmos_file_tree_item_is_directory(item) ? g_object_ref(file) : g_file_get_parent(file);

    if (target == NULL) {
        target = g_object_ref(file);
    }

    char *uri = g_file_get_uri(target);
    GtkUriLauncher *launcher = gtk_uri_launcher_new(uri);
    gtk_uri_launcher_launch(launcher, file_tree_context_window(context), NULL, NULL, NULL);

    g_object_unref(launcher);
    g_free(uri);
    g_object_unref(target);
    g_object_unref(file);
}

static char *file_tree_menu_parent_path(const char *path, const char *kind) {
    return g_strcmp0(kind, "directory") == 0 ? g_strdup(path) : g_path_get_dirname(path);
}

static FileTreeMenuAction *file_tree_menu_action_new(
    FileTreeContext *context,
    FileTreeMenuOperation operation,
    KosmosFileTreeItem *item,
    GtkWidget *popover
) {
    FileTreeMenuAction *action = g_new0(FileTreeMenuAction, 1);

    action->context = context;
    action->operation = operation;
    action->path = g_strdup(item->path);
    action->kind = g_strdup(item->kind);
    action->name = g_strdup(item->name);
    action->entries = file_tree_context_action_entries(context, item);
    action->popover = g_object_ref(popover);

    return action;
}

static void file_tree_menu_button_clicked(GtkButton *button, gpointer user_data) {
    (void)button;

    FileTreeMenuAction *action = user_data;
    gtk_popover_popdown(GTK_POPOVER(action->popover));

    switch (action->operation) {
    case FILE_TREE_MENU_NEW_FILE: {
        char *parent = file_tree_menu_parent_path(action->path, action->kind);
        show_name_dialog(action->context, FILE_TREE_DIALOG_CREATE_FILE, parent, "");
        g_free(parent);
        break;
    }
    case FILE_TREE_MENU_NEW_FOLDER: {
        char *parent = file_tree_menu_parent_path(action->path, action->kind);
        show_name_dialog(action->context, FILE_TREE_DIALOG_CREATE_DIRECTORY, parent, "");
        g_free(parent);
        break;
    }
    case FILE_TREE_MENU_RENAME:
        file_tree_context_set_focused_path(action->context, action->path);
        file_tree_context_set_editing_path(action->context, action->path);
        file_tree_context_refresh(action->context);
        break;
    case FILE_TREE_MENU_DELETE:
        show_delete_dialog(action->context, action->entries);
        break;
    case FILE_TREE_MENU_REFRESH:
        file_tree_context_refresh(action->context);
        break;
    case FILE_TREE_MENU_CUT: {
        file_tree_context_store_clipboard(action->context, action->entries, TRUE);
        break;
    }
    case FILE_TREE_MENU_COPY: {
        file_tree_context_store_clipboard(action->context, action->entries, FALSE);
        break;
    }
    case FILE_TREE_MENU_PASTE: {
        if (!file_tree_context_has_clipboard(action->context)) {
            break;
        }

        FileTreeEntryRef *first_clipboard_entry = g_ptr_array_index(action->context->clipboard_entries, 0);
        char *target_directory = file_tree_paste_target_directory(action->path, action->kind);
        char *conflict_path = file_tree_path_child(target_directory, first_clipboard_entry->name);
        gboolean was_cut = action->context->clipboard_cut;
        const char *conflict_name = was_cut ? NULL : file_tree_context_clipboard_conflict_name(action->context, target_directory);
        char *focused_path = was_cut
            ? g_strdup(conflict_path)
            : file_tree_available_copy_path(target_directory, first_clipboard_entry->name);
        FileTreeAsyncRequest *request = file_tree_async_request_new(
            action->context,
            was_cut ? FILE_TREE_ASYNC_MOVE : FILE_TREE_ASYNC_COPY,
            was_cut ? "Failed to move entry" : "Failed to copy entry"
        );

        request->entries = file_tree_entry_ref_array_copy(action->context->clipboard_entries);
        request->target_directory_path = g_strdup(target_directory);
        request->pending_focus_path = g_strdup(focused_path);
        request->show_progress = TRUE;
        request->clear_clipboard_on_success = was_cut;

        if (conflict_name != NULL) {
            file_tree_confirm_copy_conflict(action->context, request, conflict_name);
        } else {
            file_tree_async_request_start(action->context, request, was_cut ? "Moving entries..." : "Copying entries...");
        }

        g_free(focused_path);
        g_free(conflict_path);
        g_free(target_directory);
        break;
    }
    case FILE_TREE_MENU_REVEAL: {
        KosmosFileTreeItem item = {
            .name = action->name,
            .path = action->path,
            .kind = action->kind,
            .children = NULL,
        };
        reveal_file_tree_entry(action->context, &item);
        break;
    }
    }
}

static void append_file_tree_menu_item(GMenu *menu, const char *id) {
    GMenuItem *menu_item = g_menu_item_new(NULL, NULL);

    g_menu_item_set_attribute(menu_item, "custom", "s", id);
    g_menu_append_item(menu, menu_item);
    g_object_unref(menu_item);
}

static GtkWidget *create_file_tree_menu_button(
    const char *label_text,
    FileTreeContext *context,
    FileTreeMenuOperation operation,
    KosmosFileTreeItem *item,
    GtkWidget *popover
) {
    GtkWidget *button = gtk_button_new();
    GtkWidget *label = gtk_label_new(label_text);
    FileTreeMenuAction *action = file_tree_menu_action_new(context, operation, item, popover);

    gtk_label_set_xalign(GTK_LABEL(label), 0.0f);
    gtk_widget_set_hexpand(label, TRUE);
    gtk_widget_set_halign(label, GTK_ALIGN_FILL);
    gtk_widget_set_hexpand(button, TRUE);
    gtk_widget_set_halign(button, GTK_ALIGN_FILL);
    gtk_widget_add_css_class(button, "flat");
    gtk_button_set_child(GTK_BUTTON(button), label);
    g_object_set_data_full(G_OBJECT(button), "file-tree-menu-action", action, (GDestroyNotify)file_tree_menu_action_free);
    g_signal_connect(button, "clicked", G_CALLBACK(file_tree_menu_button_clicked), action);

    return button;
}

static void add_file_tree_menu_button(
    GtkPopoverMenu *popover,
    const char *id,
    const char *label,
    FileTreeContext *context,
    FileTreeMenuOperation operation,
    KosmosFileTreeItem *item
) {
    GtkWidget *button = create_file_tree_menu_button(label, context, operation, item, GTK_WIDGET(popover));
    gtk_popover_menu_add_child(popover, button, id);
}

static GMenu *create_file_tree_menu_model(FileTreeContext *context, KosmosFileTreeItem *item) {
    GMenu *menu = g_menu_new();
    GMenu *create_section = g_menu_new();
    GMenu *entry_section = g_menu_new();
    GMenu *clipboard_section = g_menu_new();
    GMenu *location_section = g_menu_new();
    GPtrArray *action_entries = file_tree_context_action_entries(context, item);
    gboolean batch = action_entries->len > 1;

    append_file_tree_menu_item(create_section, "new-file");
    append_file_tree_menu_item(create_section, "new-folder");
    g_menu_append_section(menu, NULL, G_MENU_MODEL(create_section));

    if (!file_tree_item_is_root(context, item)) {
        if (!batch) {
            append_file_tree_menu_item(entry_section, "rename");
        }
        append_file_tree_menu_item(entry_section, "cut");
        append_file_tree_menu_item(entry_section, "copy");
        append_file_tree_menu_item(entry_section, "delete");
        g_menu_append_section(menu, NULL, G_MENU_MODEL(entry_section));
    }

    if (file_tree_context_has_clipboard(context)) {
        append_file_tree_menu_item(clipboard_section, "paste");
        g_menu_append_section(menu, NULL, G_MENU_MODEL(clipboard_section));
    }

    append_file_tree_menu_item(location_section, "reveal");
    append_file_tree_menu_item(location_section, "refresh");
    g_menu_append_section(menu, NULL, G_MENU_MODEL(location_section));

    g_object_unref(create_section);
    g_object_unref(entry_section);
    g_object_unref(clipboard_section);
    g_object_unref(location_section);
    g_ptr_array_unref(action_entries);

    return menu;
}

static void show_file_tree_menu(FileTreeContext *context, KosmosFileTreeItem *item, double x, double y) {
    GMenu *menu = create_file_tree_menu_model(context, item);
    GtkWidget *popover = gtk_popover_menu_new_from_model(NULL);
    gboolean root = file_tree_item_is_root(context, item);
    GPtrArray *action_entries = file_tree_context_action_entries(context, item);
    gboolean batch = action_entries->len > 1;
    char *cut_label = batch ? g_strdup_printf("Cut %u Entries", action_entries->len) : g_strdup("Cut");
    char *copy_label = batch ? g_strdup_printf("Copy %u Entries", action_entries->len) : g_strdup("Copy");
    char *delete_label = batch ? g_strdup_printf("Delete %u Entries", action_entries->len) : g_strdup("Delete");
    GdkRectangle rectangle = {
        .x = (int)x,
        .y = (int)y,
        .width = 1,
        .height = 1,
    };

    gtk_popover_menu_set_menu_model(GTK_POPOVER_MENU(popover), G_MENU_MODEL(menu));
    add_file_tree_menu_button(GTK_POPOVER_MENU(popover), "new-file", "New File", context, FILE_TREE_MENU_NEW_FILE, item);
    add_file_tree_menu_button(GTK_POPOVER_MENU(popover), "new-folder", "New Folder", context, FILE_TREE_MENU_NEW_FOLDER, item);
    if (!root) {
        if (!batch) {
            add_file_tree_menu_button(GTK_POPOVER_MENU(popover), "rename", "Rename", context, FILE_TREE_MENU_RENAME, item);
        }
        add_file_tree_menu_button(GTK_POPOVER_MENU(popover), "cut", cut_label, context, FILE_TREE_MENU_CUT, item);
        add_file_tree_menu_button(GTK_POPOVER_MENU(popover), "copy", copy_label, context, FILE_TREE_MENU_COPY, item);
        add_file_tree_menu_button(GTK_POPOVER_MENU(popover), "delete", delete_label, context, FILE_TREE_MENU_DELETE, item);
    }
    if (file_tree_context_has_clipboard(context)) {
        add_file_tree_menu_button(GTK_POPOVER_MENU(popover), "paste", "Paste", context, FILE_TREE_MENU_PASTE, item);
    }
    add_file_tree_menu_button(GTK_POPOVER_MENU(popover), "reveal", "Reveal in Files", context, FILE_TREE_MENU_REVEAL, item);
    add_file_tree_menu_button(GTK_POPOVER_MENU(popover), "refresh", "Refresh", context, FILE_TREE_MENU_REFRESH, item);
    gtk_widget_set_parent(popover, context->root);
    gtk_popover_set_has_arrow(GTK_POPOVER(popover), FALSE);
    gtk_popover_set_pointing_to(GTK_POPOVER(popover), &rectangle);
    g_signal_connect(popover, "closed", G_CALLBACK(gtk_widget_unparent), NULL);
    gtk_popover_popup(GTK_POPOVER(popover));
    g_free(cut_label);
    g_free(copy_label);
    g_free(delete_label);
    g_ptr_array_unref(action_entries);
    g_object_unref(menu);
}

static void show_file_tree_root_menu(FileTreeContext *context, double x, double y) {
    if (context->root_path == NULL) {
        return;
    }

    KosmosFileTreeItem item = {
        .name = context->root_name == NULL ? context->root_path : context->root_name,
        .path = context->root_path,
        .kind = "directory",
        .children = NULL,
    };

    show_file_tree_menu(context, &item, x, y);
}

static void file_tree_context_menu(GtkGestureClick *gesture, int n_press, double x, double y, gpointer user_data) {
    (void)n_press;

    FileTreeContext *context = user_data;
    if (context->list == NULL) {
        return;
    }

    GtkWidget *source = gtk_event_controller_get_widget(GTK_EVENT_CONTROLLER(gesture));
    graphene_point_t source_point = GRAPHENE_POINT_INIT((float)x, (float)y);
    graphene_point_t list_point = GRAPHENE_POINT_INIT(0.0f, 0.0f);
    graphene_point_t root_point = GRAPHENE_POINT_INIT(0.0f, 0.0f);

    if (!gtk_widget_compute_point(source, context->list, &source_point, &list_point) ||
        !gtk_widget_compute_point(source, context->root, &source_point, &root_point)) {
        return;
    }

    GtkWidget *picked = gtk_widget_pick(context->list, list_point.x, list_point.y, GTK_PICK_DEFAULT);
    GtkTreeListRow *tree_row = tree_row_for_picked_widget(picked);
    if (tree_row == NULL) {
        show_file_tree_root_menu(context, root_point.x, root_point.y);
        return;
    }

    KosmosFileTreeItem *item = KOSMOS_FILE_TREE_ITEM(gtk_tree_list_row_get_item(tree_row));
    if (!kosmos_file_tree_item_is_message(item)) {
        gboolean selected = file_tree_context_path_is_selected(context, item->path);
        file_tree_context_set_focused_path(context, item->path);
        if (!selected) {
            file_tree_context_select_focused_path(context);
        }
        show_file_tree_menu(context, item, root_point.x, root_point.y);
    }

    g_object_unref(tree_row);
}

static void unbind_list_item(GtkSignalListItemFactory *factory, GtkListItem *list_item, gpointer user_data) {
    (void)factory;
    (void)user_data;

    GtkWidget *hit_row = gtk_list_item_get_child(list_item);
    if (hit_row != NULL) {
        GtkWidget *entry = g_object_get_data(G_OBJECT(hit_row), "entry-editor");

        g_object_set_data(G_OBJECT(hit_row), "tree-row", NULL);
        g_object_set_data(G_OBJECT(hit_row), "file-tree-context", NULL);
        g_object_set_data(G_OBJECT(hit_row), "file-tree-item", NULL);
        gtk_widget_remove_css_class(hit_row, "file-tree-drop-target");

        if (entry != NULL) {
            g_object_set_data(G_OBJECT(entry), "file-tree-context", NULL);
            g_object_set_data(G_OBJECT(entry), "file-tree-item", NULL);
        }
    }
}

static GtkWidget *create_file_tree_list(FileTreeContext *context, JsonObject *root) {
    GListStore *root_store = g_list_store_new(KOSMOS_TYPE_FILE_TREE_ITEM);
    KosmosFileTreeItem *root_item = file_tree_item_from_entry(root);
    g_list_store_append(root_store, root_item);
    file_tree_context_rebuild_monitors(context, root_item);
    g_object_unref(root_item);

    GtkTreeListModel *tree_model = gtk_tree_list_model_new(
        G_LIST_MODEL(root_store),
        FALSE,
        FALSE,
        create_child_model,
        NULL,
        NULL
    );
    GtkTreeListRow *root_row = g_list_model_get_item(G_LIST_MODEL(tree_model), 0);
    if (root_row != NULL) {
        gtk_tree_list_row_set_expanded(root_row, TRUE);
        g_object_unref(root_row);
    }
    file_tree_context_restore_expanded(context, tree_model);
    context->tree_model = g_object_ref(tree_model);

    GtkListItemFactory *factory = gtk_signal_list_item_factory_new();
    g_signal_connect(factory, "setup", G_CALLBACK(setup_list_item), NULL);
    g_signal_connect(factory, "bind", G_CALLBACK(bind_list_item), context);
    g_signal_connect(factory, "unbind", G_CALLBACK(unbind_list_item), NULL);

    GtkMultiSelection *selection = gtk_multi_selection_new(G_LIST_MODEL(tree_model));

    guint focused_position = 0;
    if (file_tree_model_index_for_path(tree_model, context->focused_path, &focused_position)) {
        gtk_selection_model_select_item(GTK_SELECTION_MODEL(selection), focused_position, TRUE);
    }

    GtkWidget *list = gtk_list_view_new(GTK_SELECTION_MODEL(selection), factory);
    GtkGesture *click = gtk_gesture_click_new();
    GtkGesture *context_menu = gtk_gesture_click_new();

    gtk_widget_set_margin_bottom(list, 8);
    gtk_widget_set_vexpand(list, TRUE);
    gtk_widget_set_hexpand(list, TRUE);
    gtk_gesture_single_set_button(GTK_GESTURE_SINGLE(click), GDK_BUTTON_PRIMARY);
    gtk_event_controller_set_propagation_phase(GTK_EVENT_CONTROLLER(click), GTK_PHASE_CAPTURE);
    g_signal_connect(click, "pressed", G_CALLBACK(file_tree_clicked), context);
    gtk_widget_add_controller(list, GTK_EVENT_CONTROLLER(click));

    gtk_gesture_single_set_button(GTK_GESTURE_SINGLE(context_menu), GDK_BUTTON_SECONDARY);
    gtk_event_controller_set_propagation_phase(GTK_EVENT_CONTROLLER(context_menu), GTK_PHASE_CAPTURE);
    g_signal_connect(context_menu, "pressed", G_CALLBACK(file_tree_context_menu), context);
    gtk_widget_add_controller(list, GTK_EVENT_CONTROLLER(context_menu));

    return list;
}

static GtkWidget *create_file_tree_root(FileTreeContext *context) {
    GtkWidget *root_widget = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    GtkWidget *notice_label = gtk_label_new(NULL);
    GtkWidget *scrolled = gtk_scrolled_window_new();

    context->root = root_widget;
    context->notice_label = notice_label;
    context->scrolled = scrolled;
    gtk_widget_set_hexpand(root_widget, TRUE);
    gtk_widget_set_vexpand(root_widget, TRUE);
    file_tree_install_css(root_widget);

    gtk_label_set_xalign(GTK_LABEL(notice_label), 0.0f);
    gtk_label_set_wrap(GTK_LABEL(notice_label), TRUE);
    gtk_widget_set_margin_top(notice_label, 8);
    gtk_widget_set_margin_bottom(notice_label, 4);
    gtk_widget_set_margin_start(notice_label, 10);
    gtk_widget_set_margin_end(notice_label, 10);
    gtk_widget_set_visible(notice_label, FALSE);

    gtk_scrolled_window_set_policy(GTK_SCROLLED_WINDOW(scrolled), GTK_POLICY_AUTOMATIC, GTK_POLICY_AUTOMATIC);
    gtk_widget_set_hexpand(scrolled, TRUE);
    gtk_widget_set_vexpand(scrolled, TRUE);
    gtk_box_append(GTK_BOX(root_widget), notice_label);
    gtk_box_append(GTK_BOX(root_widget), scrolled);
    g_object_set_data_full(G_OBJECT(root_widget), "file-tree-context", context, (GDestroyNotify)file_tree_context_free);

    return root_widget;
}

GtkWidget *kosmos_file_tree_tab_create(KosmosIpcClient *ipc_client, guint64 workspace_id) {
    g_return_val_if_fail(KOSMOS_IS_IPC_CLIENT(ipc_client), NULL);

    FileTreeContext *context = g_new0(FileTreeContext, 1);
    context->ipc_client = g_object_ref(ipc_client);
    context->workspace_id = workspace_id;
    GtkWidget *view = create_file_tree_root(context);
    file_tree_context_refresh(context);

    return view;
}
