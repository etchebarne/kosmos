#include "ui/kosmos_main_window_private.h"

void kosmos_workspace_switcher_clear(KosmosMainWindow *self) {
    GtkWidget *child = gtk_widget_get_first_child(self->workspace_switcher);

    while (child != NULL) {
        GtkWidget *next = gtk_widget_get_next_sibling(child);
        gtk_box_remove(GTK_BOX(self->workspace_switcher), child);
        child = next;
    }

    g_hash_table_remove_all(self->workspace_buttons);
    self->add_workspace_button = NULL;
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

static void activate_workspace(GtkWidget *button, gpointer user_data) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    guint64 *workspace_id = g_object_get_data(G_OBJECT(button), "workspace-id");

    if (workspace_id == NULL || !kosmos_main_window_ensure_connected(self)) {
        return;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    if (!kosmos_ipc_client_activate_workspace(self->ipc_client, *workspace_id, &state, NULL, &error)) {
        kosmos_main_window_set_error_status(self, "Failed to activate workspace", error);
        g_clear_error(&error);
        return;
    }

    kosmos_main_window_apply_workspace_state(self, state);
    json_node_unref(state);
}

static void open_workspace_path(KosmosMainWindow *self, const char *path) {
    if (!kosmos_main_window_ensure_connected(self)) {
        return;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    if (!kosmos_ipc_client_open_workspace(self->ipc_client, path, &state, NULL, &error)) {
        kosmos_main_window_set_error_status(self, "Failed to open workspace", error);
        g_clear_error(&error);
        return;
    }

    kosmos_main_window_apply_workspace_state(self, state);
    json_node_unref(state);
}

static void open_workspace_selected(GObject *source_object, GAsyncResult *result, gpointer user_data) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    GError *error = NULL;
    GFile *folder = gtk_file_dialog_select_folder_finish(GTK_FILE_DIALOG(source_object), result, &error);

    if (folder == NULL) {
        if (!g_error_matches(error, GTK_DIALOG_ERROR, GTK_DIALOG_ERROR_DISMISSED)) {
            kosmos_main_window_set_error_status(self, "Failed to select workspace", error);
        }

        g_clear_error(&error);
        g_object_unref(self);
        return;
    }

    char *path = g_file_get_path(folder);
    if (path == NULL) {
        kosmos_main_window_set_status(self, "Selected workspace is not a local directory.");
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

GtkWidget *kosmos_workspace_switcher_ensure_add_button(KosmosMainWindow *self, gboolean sensitive) {
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

static GtkWidget *update_workspace_button(
    KosmosMainWindow *self,
    JsonObject *workspace,
    guint64 active_workspace_id,
    gboolean has_active_workspace
) {
    guint64 workspace_id = 0;
    const char *name = NULL;

    if (!kosmos_json_get_uint_member(workspace, "id", &workspace_id) ||
        !kosmos_json_get_string_member(workspace, "name", &name)) {
        return NULL;
    }

    GtkWidget *button = workspace_button_for(self, workspace_id, name);
    set_workspace_button_active(button, has_active_workspace && workspace_id == active_workspace_id);

    return button;
}

static JsonObject *find_active_workspace(JsonArray *workspaces, guint64 active_workspace_id, gboolean has_active_workspace) {
    if (!has_active_workspace) {
        return NULL;
    }

    guint workspace_count = json_array_get_length(workspaces);
    for (guint index = 0; index < workspace_count; index++) {
        JsonNode *workspace_node = json_array_get_element(workspaces, index);
        if (workspace_node == NULL || !JSON_NODE_HOLDS_OBJECT(workspace_node)) {
            continue;
        }

        JsonObject *workspace = json_node_get_object(workspace_node);
        guint64 workspace_id = 0;
        if (kosmos_json_get_uint_member(workspace, "id", &workspace_id) && workspace_id == active_workspace_id) {
            return workspace;
        }
    }

    return NULL;
}

void kosmos_main_window_apply_workspace_state(KosmosMainWindow *self, JsonNode *state) {
    if (state == NULL || !JSON_NODE_HOLDS_OBJECT(state)) {
        kosmos_main_window_set_status(self, "Workspace state is unavailable.");
        return;
    }

    JsonObject *result = json_node_get_object(state);
    JsonNode *workspaces_node = json_object_get_member(result, "workspaces");
    if (workspaces_node == NULL || !JSON_NODE_HOLDS_ARRAY(workspaces_node)) {
        kosmos_main_window_set_status(self, "Workspace state is missing workspaces.");
        return;
    }

    guint64 active_workspace_id = 0;
    gboolean has_active_workspace = get_active_workspace_id(result, &active_workspace_id);
    JsonArray *workspaces = json_node_get_array(workspaces_node);
    guint workspace_count = json_array_get_length(workspaces);
    JsonObject *active_workspace = find_active_workspace(workspaces, active_workspace_id, has_active_workspace);
    GHashTable *seen_workspace_ids = g_hash_table_new_full(g_int64_hash, g_int64_equal, g_free, NULL);
    GtkWidget *previous_button = NULL;

    for (guint index = 0; index < workspace_count; index++) {
        JsonNode *workspace_node = json_array_get_element(workspaces, index);
        if (workspace_node != NULL && JSON_NODE_HOLDS_OBJECT(workspace_node)) {
            JsonObject *workspace = json_node_get_object(workspace_node);
            guint64 workspace_id = 0;

            if (!kosmos_json_get_uint_member(workspace, "id", &workspace_id)) {
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
    gtk_box_reorder_child_after(
        GTK_BOX(self->workspace_switcher),
        kosmos_workspace_switcher_ensure_add_button(self, TRUE),
        previous_button
    );
    g_hash_table_unref(seen_workspace_ids);

    (void)workspace_count;
    if (active_workspace != NULL) {
        char *layout_signature = kosmos_pane_layout_create_signature(active_workspace);
        if (layout_signature != NULL &&
            self->layout_signature != NULL &&
            g_strcmp0(layout_signature, self->layout_signature) == 0 &&
            kosmos_pane_layout_update_active_workspace_in_place(self, active_workspace)) {
            g_free(layout_signature);
            return;
        }

        kosmos_pane_layout_render_active_workspace(self, active_workspace);
        g_clear_pointer(&self->layout_signature, g_free);
        self->layout_signature = layout_signature;
        return;
    }

    kosmos_pane_layout_render_active_workspace(self, active_workspace);
}

GtkWidget *kosmos_workspace_switcher_create(KosmosMainWindow *self) {
    self->workspace_switcher = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
    gtk_widget_add_css_class(self->workspace_switcher, "linked");
    gtk_widget_set_halign(self->workspace_switcher, GTK_ALIGN_CENTER);
    kosmos_workspace_switcher_ensure_add_button(self, FALSE);

    return self->workspace_switcher;
}
