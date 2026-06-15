#include "ui/kosmos_main_window_private.h"

G_DEFINE_FINAL_TYPE(KosmosMainWindow, kosmos_main_window, GTK_TYPE_APPLICATION_WINDOW)

GtkWidget *kosmos_main_window_create_label(const char *text, const char *css_class) {
    GtkWidget *label = gtk_label_new(text);
    gtk_label_set_wrap(GTK_LABEL(label), TRUE);
    gtk_label_set_xalign(GTK_LABEL(label), 0.0f);

    if (css_class != NULL) {
        gtk_widget_add_css_class(label, css_class);
    }

    return label;
}

void kosmos_main_window_clear_content_area(KosmosMainWindow *self) {
    self->suppress_tab_detach = TRUE;

    if (self->pane_views != NULL) {
        g_hash_table_remove_all(self->pane_views);
    }
    g_clear_pointer(&self->layout_signature, g_free);

    if (self->staged_content_area != NULL) {
        gtk_overlay_remove_overlay(GTK_OVERLAY(self->content_overlay), self->staged_content_area);
        self->staged_content_area = NULL;
    }
    self->pending_layout_applies = 0;
    self->hiding_layout_apply = FALSE;
    self->has_rendered_workspace = FALSE;
    gtk_widget_set_opacity(self->content_area, 1.0);

    GtkWidget *child = gtk_widget_get_first_child(self->content_area);

    while (child != NULL) {
        GtkWidget *next = gtk_widget_get_next_sibling(child);
        gtk_box_remove(GTK_BOX(self->content_area), child);
        child = next;
    }

    self->suppress_tab_detach = FALSE;
}

void kosmos_main_window_set_uint64_data(GObject *object, const char *key, guint64 value) {
    guint64 *stored_value = g_new(guint64, 1);
    *stored_value = value;
    g_object_set_data_full(object, key, stored_value, g_free);
}

gboolean kosmos_main_window_get_uint64_data(GObject *object, const char *key, guint64 *value) {
    guint64 *stored_value = g_object_get_data(object, key);

    if (stored_value == NULL) {
        return FALSE;
    }

    *value = *stored_value;
    return TRUE;
}

void kosmos_main_window_register_pane_view(KosmosMainWindow *self, guint64 pane_id, AdwTabView *view) {
    guint64 *key = g_new(guint64, 1);
    *key = pane_id;
    g_hash_table_replace(self->pane_views, key, view);
}

AdwTabView *kosmos_main_window_pane_view_for(KosmosMainWindow *self, guint64 pane_id) {
    return g_hash_table_lookup(self->pane_views, &pane_id);
}

gboolean kosmos_json_get_uint_member(JsonObject *object, const char *name, guint64 *value) {
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

gboolean kosmos_json_get_string_member(JsonObject *object, const char *name, const char **value) {
    JsonNode *node = json_object_get_member(object, name);
    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node) || json_node_get_value_type(node) != G_TYPE_STRING) {
        return FALSE;
    }

    *value = json_node_get_string(node);
    return TRUE;
}

gboolean kosmos_json_get_double_member(JsonObject *object, const char *name, double *value) {
    JsonNode *node = json_object_get_member(object, name);
    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node)) {
        return FALSE;
    }

    *value = json_node_get_double(node);
    return TRUE;
}

JsonObject *kosmos_json_get_object_member(JsonObject *object, const char *name) {
    JsonNode *node = json_object_get_member(object, name);

    if (node == NULL || !JSON_NODE_HOLDS_OBJECT(node)) {
        return NULL;
    }

    return json_node_get_object(node);
}

JsonArray *kosmos_json_get_array_member(JsonObject *object, const char *name) {
    JsonNode *node = json_object_get_member(object, name);

    if (node == NULL || !JSON_NODE_HOLDS_ARRAY(node)) {
        return NULL;
    }

    return json_node_get_array(node);
}

void kosmos_main_window_set_status(KosmosMainWindow *self, const char *status) {
    kosmos_main_window_clear_content_area(self);

    if (status == NULL || status[0] == '\0') {
        return;
    }

    GtkWidget *label = kosmos_main_window_create_label(status, "dim-label");
    gtk_widget_set_halign(label, GTK_ALIGN_CENTER);
    gtk_widget_set_valign(label, GTK_ALIGN_CENTER);
    gtk_widget_set_vexpand(label, TRUE);
    gtk_box_append(GTK_BOX(self->content_area), label);
}

void kosmos_main_window_set_error_status(KosmosMainWindow *self, const char *prefix, GError *error) {
    char *message = g_strdup_printf("%s: %s", prefix, error->message);
    kosmos_main_window_set_status(self, message);
    g_free(message);
}

gboolean kosmos_main_window_ensure_connected(KosmosMainWindow *self) {
    GError *error = NULL;

    if (kosmos_ipc_client_connect(self->ipc_client, NULL, &error)) {
        return TRUE;
    }

    kosmos_main_window_set_status(self, "Start kosmos-server with ./scripts/run.sh to load workspaces.");
    kosmos_workspace_switcher_clear(self);
    g_clear_error(&error);

    return FALSE;
}

void kosmos_main_window_apply_server_state_or_show_error(
    KosmosMainWindow *self,
    JsonNode *state,
    GError *error,
    const char *error_prefix
) {
    if (error != NULL) {
        kosmos_main_window_set_error_status(self, error_prefix, error);
        return;
    }

    kosmos_main_window_apply_workspace_state(self, state);
}

static GtkWidget *create_header(KosmosMainWindow *self) {
    GtkWidget *header = gtk_header_bar_new();
    gtk_header_bar_set_title_widget(GTK_HEADER_BAR(header), kosmos_workspace_switcher_create(self));

    return header;
}

static void kosmos_main_window_init(KosmosMainWindow *self) {
    self->workspace_buttons = g_hash_table_new_full(g_int64_hash, g_int64_equal, g_free, NULL);
    self->pane_views = g_hash_table_new_full(g_int64_hash, g_int64_equal, g_free, NULL);
    kosmos_pane_dnd_install_css(GTK_WIDGET(self));

    gtk_window_set_title(GTK_WINDOW(self), "Kosmos");
    gtk_window_set_default_size(GTK_WINDOW(self), 1000, 700);

    GtkWidget *root = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    gtk_window_set_child(GTK_WINDOW(self), root);
    gtk_window_set_titlebar(GTK_WINDOW(self), create_header(self));

    self->content_overlay = gtk_overlay_new();
    gtk_widget_set_hexpand(self->content_overlay, TRUE);
    gtk_widget_set_vexpand(self->content_overlay, TRUE);

    self->content_area = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    gtk_widget_set_hexpand(self->content_area, TRUE);
    gtk_widget_set_vexpand(self->content_area, TRUE);
    gtk_overlay_set_child(GTK_OVERLAY(self->content_overlay), self->content_area);
    gtk_box_append(GTK_BOX(root), self->content_overlay);
}

static void kosmos_main_window_finalize(GObject *object) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(object);

    kosmos_pane_dnd_clear_detached_tab_transfer(self);
    g_clear_object(&self->ipc_client);
    g_clear_pointer(&self->workspace_buttons, g_hash_table_unref);
    g_clear_pointer(&self->pane_views, g_hash_table_unref);
    g_clear_pointer(&self->layout_signature, g_free);

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

    if (!kosmos_main_window_ensure_connected(self)) {
        kosmos_workspace_switcher_ensure_add_button(self, FALSE);
        return;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    if (!kosmos_ipc_client_list_workspaces(self->ipc_client, &state, NULL, &error)) {
        kosmos_main_window_set_error_status(self, "Failed to load workspaces", error);
        g_clear_error(&error);
        return;
    }

    kosmos_main_window_apply_workspace_state(self, state);
    json_node_unref(state);
}
