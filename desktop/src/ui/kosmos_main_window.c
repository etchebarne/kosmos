#include "ui/kosmos_main_window.h"

struct _KosmosMainWindow {
    GtkApplicationWindow parent_instance;
    KosmosIpcClient *ipc_client;
    GtkWidget *workspace_switcher;
    GtkWidget *add_workspace_button;
    GtkWidget *status_label;
    GHashTable *workspace_buttons;
};

G_DEFINE_FINAL_TYPE(KosmosMainWindow, kosmos_main_window, GTK_TYPE_APPLICATION_WINDOW)

static GtkWidget *create_label(const char *text, const char *css_class) {
    GtkWidget *label = gtk_label_new(text);
    gtk_label_set_wrap(GTK_LABEL(label), TRUE);
    gtk_label_set_xalign(GTK_LABEL(label), 0.0f);

    if (css_class != NULL) {
        gtk_widget_add_css_class(label, css_class);
    }

    return label;
}

static void clear_workspace_switcher(KosmosMainWindow *self) {
    GtkWidget *child = gtk_widget_get_first_child(self->workspace_switcher);

    while (child != NULL) {
        GtkWidget *next = gtk_widget_get_next_sibling(child);
        gtk_box_remove(GTK_BOX(self->workspace_switcher), child);
        child = next;
    }

    g_hash_table_remove_all(self->workspace_buttons);
    self->add_workspace_button = NULL;
}

static gboolean get_uint_member(JsonObject *object, const char *name, guint64 *value) {
    JsonNode *node = json_object_get_member(object, name);
    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node)) {
        return FALSE;
    }

    gint64 signed_value = json_node_get_int(node);
    if (signed_value < 0) {
        return FALSE;
    }

    *value = (guint64)signed_value;
    return TRUE;
}

static gboolean get_string_member(JsonObject *object, const char *name, const char **value) {
    JsonNode *node = json_object_get_member(object, name);
    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node) || json_node_get_value_type(node) != G_TYPE_STRING) {
        return FALSE;
    }

    *value = json_node_get_string(node);
    return TRUE;
}

static gboolean get_active_workspace_id(JsonObject *result, guint64 *active_workspace_id) {
    JsonNode *node = json_object_get_member(result, "activeWorkspaceId");
    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node)) {
        return FALSE;
    }

    gint64 signed_value = json_node_get_int(node);
    if (signed_value < 0) {
        return FALSE;
    }

    *active_workspace_id = (guint64)signed_value;
    return TRUE;
}

static void set_status(KosmosMainWindow *self, const char *status) {
    gtk_label_set_text(GTK_LABEL(self->status_label), status);
}

static void set_error_status(KosmosMainWindow *self, const char *prefix, GError *error) {
    char *message = g_strdup_printf("%s: %s", prefix, error->message);
    set_status(self, message);
    g_free(message);
}

static gboolean ensure_connected(KosmosMainWindow *self) {
    GError *error = NULL;

    if (kosmos_ipc_client_connect(self->ipc_client, NULL, &error)) {
        return TRUE;
    }

    set_status(self, "Start kosmos-server with ./scripts/run.sh to load workspaces.");
    clear_workspace_switcher(self);
    g_clear_error(&error);

    return FALSE;
}

static void apply_workspace_state(KosmosMainWindow *self, JsonNode *state);

static void activate_workspace(GtkWidget *button, gpointer user_data) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    guint64 *workspace_id = g_object_get_data(G_OBJECT(button), "workspace-id");

    if (workspace_id == NULL || !ensure_connected(self)) {
        return;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    if (!kosmos_ipc_client_activate_workspace(self->ipc_client, *workspace_id, &state, NULL, &error)) {
        set_error_status(self, "Failed to activate workspace", error);
        g_clear_error(&error);
        return;
    }

    apply_workspace_state(self, state);
    json_node_unref(state);
}

static void open_workspace_path(KosmosMainWindow *self, const char *path) {
    if (!ensure_connected(self)) {
        return;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    if (!kosmos_ipc_client_open_workspace(self->ipc_client, path, &state, NULL, &error)) {
        set_error_status(self, "Failed to open workspace", error);
        g_clear_error(&error);
        return;
    }

    apply_workspace_state(self, state);
    json_node_unref(state);
}

static void open_workspace_selected(GObject *source_object, GAsyncResult *result, gpointer user_data) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    GError *error = NULL;
    GFile *folder = gtk_file_dialog_select_folder_finish(GTK_FILE_DIALOG(source_object), result, &error);

    if (folder == NULL) {
        if (!g_error_matches(error, GTK_DIALOG_ERROR, GTK_DIALOG_ERROR_DISMISSED)) {
            set_error_status(self, "Failed to select workspace", error);
        }

        g_clear_error(&error);
        g_object_unref(self);
        return;
    }

    char *path = g_file_get_path(folder);
    if (path == NULL) {
        set_status(self, "Selected workspace is not a local directory.");
    } else {
        open_workspace_path(self, path);
    }

    g_free(path);
    g_object_unref(folder);
    g_object_unref(self);
}

static void open_workspace_picker(GtkButton *button, gpointer user_data) {
    (void)button;

    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    GtkFileDialog *dialog = gtk_file_dialog_new();
    gtk_file_dialog_set_title(dialog, "Open Workspace");
    gtk_file_dialog_select_folder(dialog, GTK_WINDOW(self), NULL, open_workspace_selected, g_object_ref(self));
    g_object_unref(dialog);
}

static GtkWidget *ensure_add_workspace_button(KosmosMainWindow *self, gboolean sensitive) {
    if (self->add_workspace_button == NULL) {
        self->add_workspace_button = gtk_button_new_from_icon_name("list-add-symbolic");
        gtk_widget_set_tooltip_text(self->add_workspace_button, "Open workspace");
        gtk_box_append(GTK_BOX(self->workspace_switcher), self->add_workspace_button);
        g_signal_connect(self->add_workspace_button, "clicked", G_CALLBACK(open_workspace_picker), self);
    }

    gtk_widget_set_sensitive(self->add_workspace_button, sensitive);
    return self->add_workspace_button;
}

static void split_workspace_name(const char *name, char **initial, char **suffix) {
    if (name == NULL || name[0] == '\0') {
        *initial = g_strdup("?");
        *suffix = g_strdup("");
        return;
    }

    const char *suffix_start = g_utf8_next_char(name);
    *initial = g_strndup(name, suffix_start - name);
    *suffix = g_strdup(suffix_start);
}

static void update_workspace_button_name(GtkWidget *button, const char *name) {
    GtkWidget *initial_label = g_object_get_data(G_OBJECT(button), "workspace-initial-label");
    GtkWidget *suffix_label = g_object_get_data(G_OBJECT(button), "workspace-suffix-label");
    char *initial = NULL;
    char *suffix = NULL;

    split_workspace_name(name, &initial, &suffix);
    gtk_label_set_text(GTK_LABEL(initial_label), initial);
    gtk_label_set_text(GTK_LABEL(suffix_label), suffix);

    g_free(initial);
    g_free(suffix);
}

static void set_workspace_button_active(GtkWidget *button, gboolean active) {
    GtkWidget *revealer = g_object_get_data(G_OBJECT(button), "workspace-suffix-revealer");

    gtk_toggle_button_set_active(GTK_TOGGLE_BUTTON(button), active);
    gtk_revealer_set_reveal_child(GTK_REVEALER(revealer), active);
}

static GtkWidget *create_workspace_button(guint64 workspace_id, const char *name) {
    GtkWidget *button = gtk_toggle_button_new();
    GtkWidget *label_box = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
    GtkWidget *initial_label = gtk_label_new(NULL);
    GtkWidget *suffix_label = gtk_label_new(NULL);
    GtkWidget *revealer = gtk_revealer_new();

    gtk_widget_set_halign(label_box, GTK_ALIGN_CENTER);
    gtk_widget_set_valign(label_box, GTK_ALIGN_CENTER);
    gtk_label_set_xalign(GTK_LABEL(initial_label), 0.5f);

    gtk_revealer_set_transition_type(GTK_REVEALER(revealer), GTK_REVEALER_TRANSITION_TYPE_SLIDE_RIGHT);
    gtk_revealer_set_transition_duration(GTK_REVEALER(revealer), 180);
    gtk_revealer_set_child(GTK_REVEALER(revealer), suffix_label);

    gtk_box_append(GTK_BOX(label_box), initial_label);
    gtk_box_append(GTK_BOX(label_box), revealer);
    gtk_button_set_child(GTK_BUTTON(button), label_box);

    g_object_set_data(G_OBJECT(button), "workspace-initial-label", initial_label);
    g_object_set_data(G_OBJECT(button), "workspace-suffix-label", suffix_label);
    g_object_set_data(G_OBJECT(button), "workspace-suffix-revealer", revealer);

    guint64 *button_workspace_id = g_new(guint64, 1);
    *button_workspace_id = workspace_id;
    g_object_set_data_full(G_OBJECT(button), "workspace-id", button_workspace_id, g_free);

    update_workspace_button_name(button, name);

    return button;
}

static GtkWidget *workspace_button_for(KosmosMainWindow *self, guint64 workspace_id, const char *name) {
    GtkWidget *button = g_hash_table_lookup(self->workspace_buttons, &workspace_id);

    if (button != NULL) {
        update_workspace_button_name(button, name);
        return button;
    }

    button = create_workspace_button(workspace_id, name);
    guint64 *key = g_new(guint64, 1);
    *key = workspace_id;
    g_hash_table_insert(self->workspace_buttons, key, button);
    gtk_box_append(GTK_BOX(self->workspace_switcher), button);
    g_signal_connect(button, "clicked", G_CALLBACK(activate_workspace), self);

    return button;
}

static void remove_stale_workspace_buttons(KosmosMainWindow *self, GHashTable *seen_workspace_ids) {
    GHashTableIter iter;
    gpointer key = NULL;
    gpointer value = NULL;

    g_hash_table_iter_init(&iter, self->workspace_buttons);
    while (g_hash_table_iter_next(&iter, &key, &value)) {
        if (!g_hash_table_contains(seen_workspace_ids, key)) {
            gtk_box_remove(GTK_BOX(self->workspace_switcher), GTK_WIDGET(value));
            g_hash_table_iter_remove(&iter);
        }
    }
}

static GtkWidget *update_workspace_button(KosmosMainWindow *self, JsonObject *workspace, guint64 active_workspace_id, gboolean has_active_workspace) {
    guint64 workspace_id = 0;
    const char *name = NULL;

    if (!get_uint_member(workspace, "id", &workspace_id) || !get_string_member(workspace, "name", &name)) {
        return NULL;
    }

    GtkWidget *button = workspace_button_for(self, workspace_id, name);
    set_workspace_button_active(button, has_active_workspace && workspace_id == active_workspace_id);

    return button;
}

static void apply_workspace_state(KosmosMainWindow *self, JsonNode *state) {
    if (state == NULL || !JSON_NODE_HOLDS_OBJECT(state)) {
        set_status(self, "Workspace state is unavailable.");
        return;
    }

    JsonObject *result = json_node_get_object(state);
    JsonNode *workspaces_node = json_object_get_member(result, "workspaces");
    if (workspaces_node == NULL || !JSON_NODE_HOLDS_ARRAY(workspaces_node)) {
        set_status(self, "Workspace state is missing workspaces.");
        return;
    }

    guint64 active_workspace_id = 0;
    gboolean has_active_workspace = get_active_workspace_id(result, &active_workspace_id);
    JsonArray *workspaces = json_node_get_array(workspaces_node);
    guint workspace_count = json_array_get_length(workspaces);
    GHashTable *seen_workspace_ids = g_hash_table_new_full(g_int64_hash, g_int64_equal, g_free, NULL);
    GtkWidget *previous_button = NULL;

    for (guint index = 0; index < workspace_count; index++) {
        JsonNode *workspace_node = json_array_get_element(workspaces, index);
        if (workspace_node != NULL && JSON_NODE_HOLDS_OBJECT(workspace_node)) {
            JsonObject *workspace = json_node_get_object(workspace_node);
            guint64 workspace_id = 0;

            if (!get_uint_member(workspace, "id", &workspace_id)) {
                continue;
            }

            guint64 *seen_id = g_new(guint64, 1);
            *seen_id = workspace_id;
            g_hash_table_add(seen_workspace_ids, seen_id);

            GtkWidget *button = update_workspace_button(self, workspace, active_workspace_id, has_active_workspace);
            if (button != NULL) {
                gtk_box_reorder_child_after(GTK_BOX(self->workspace_switcher), button, previous_button);
                previous_button = button;
            }
        }
    }

    remove_stale_workspace_buttons(self, seen_workspace_ids);
    gtk_box_reorder_child_after(GTK_BOX(self->workspace_switcher), ensure_add_workspace_button(self, TRUE), previous_button);
    g_hash_table_unref(seen_workspace_ids);

    if (workspace_count == 0) {
        set_status(self, "No workspaces open. Use the plus button in the header to open one.");
        return;
    }

    set_status(self, has_active_workspace ? "Workspace state synced from kosmos-server." : "No active workspace selected.");
}

static GtkWidget *create_workspace_switcher(KosmosMainWindow *self) {
    self->workspace_switcher = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
    gtk_widget_add_css_class(self->workspace_switcher, "linked");
    gtk_widget_set_halign(self->workspace_switcher, GTK_ALIGN_CENTER);
    ensure_add_workspace_button(self, FALSE);

    return self->workspace_switcher;
}

static GtkWidget *create_header(KosmosMainWindow *self) {
    GtkWidget *header = gtk_header_bar_new();
    gtk_header_bar_set_title_widget(GTK_HEADER_BAR(header), create_workspace_switcher(self));

    return header;
}

static void kosmos_main_window_init(KosmosMainWindow *self) {
    self->workspace_buttons = g_hash_table_new_full(g_int64_hash, g_int64_equal, g_free, NULL);

    gtk_window_set_title(GTK_WINDOW(self), "Kosmos");
    gtk_window_set_default_size(GTK_WINDOW(self), 1000, 700);

    GtkWidget *root = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    gtk_window_set_child(GTK_WINDOW(self), root);
    gtk_window_set_titlebar(GTK_WINDOW(self), create_header(self));

    GtkWidget *content = gtk_box_new(GTK_ORIENTATION_VERTICAL, 16);
    gtk_widget_set_margin_top(content, 64);
    gtk_widget_set_margin_bottom(content, 64);
    gtk_widget_set_margin_start(content, 64);
    gtk_widget_set_margin_end(content, 64);
    gtk_widget_set_vexpand(content, TRUE);
    gtk_box_append(GTK_BOX(root), content);

    self->status_label = create_label("Loading workspace state...", "dim-label");
    gtk_widget_set_halign(self->status_label, GTK_ALIGN_CENTER);
    gtk_widget_set_valign(self->status_label, GTK_ALIGN_CENTER);
    gtk_widget_set_vexpand(self->status_label, TRUE);
    gtk_box_append(GTK_BOX(content), self->status_label);
}

static void kosmos_main_window_finalize(GObject *object) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(object);

    g_clear_object(&self->ipc_client);
    g_clear_pointer(&self->workspace_buttons, g_hash_table_unref);

    G_OBJECT_CLASS(kosmos_main_window_parent_class)->finalize(object);
}

static void kosmos_main_window_class_init(KosmosMainWindowClass *klass) {
    GObjectClass *object_class = G_OBJECT_CLASS(klass);
    object_class->finalize = kosmos_main_window_finalize;
}

GtkWidget *kosmos_main_window_new(GtkApplication *application, KosmosIpcClient *ipc_client) {
    g_return_val_if_fail(GTK_IS_APPLICATION(application), NULL);
    g_return_val_if_fail(KOSMOS_IS_IPC_CLIENT(ipc_client), NULL);

    KosmosMainWindow *self = g_object_new(
        KOSMOS_TYPE_MAIN_WINDOW,
        "application", application,
        NULL
    );

    self->ipc_client = g_object_ref(ipc_client);

    return GTK_WIDGET(self);
}

void kosmos_main_window_refresh_workspace_state(KosmosMainWindow *self) {
    g_return_if_fail(KOSMOS_IS_MAIN_WINDOW(self));

    if (!ensure_connected(self)) {
        ensure_add_workspace_button(self, FALSE);
        return;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    if (!kosmos_ipc_client_list_workspaces(self->ipc_client, &state, NULL, &error)) {
        set_error_status(self, "Failed to load workspaces", error);
        g_clear_error(&error);
        return;
    }

    apply_workspace_state(self, state);
    json_node_unref(state);
}
