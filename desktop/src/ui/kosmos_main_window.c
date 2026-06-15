#include "ui/kosmos_main_window.h"

#include <adwaita.h>

struct _KosmosMainWindow {
    GtkApplicationWindow parent_instance;
    KosmosIpcClient *ipc_client;
    GtkWidget *workspace_switcher;
    GtkWidget *add_workspace_button;
    GtkWidget *content_overlay;
    GtkWidget *content_area;
    GHashTable *workspace_buttons;
    GHashTable *paned_ratios;
    GHashTable *pane_views;
    char *layout_signature;
    AdwTabView *detached_tab_sink;
    GtkWindow *detached_tab_window;
    gboolean splitting_detached_tab;
    gboolean suppress_tab_detach;
    gboolean applying_server_state;
};

typedef struct {
    KosmosMainWindow *window;
    guint64 workspace_id;
    guint64 pane_id;
    guint64 target_pane_id;
    guint64 tab_id;
    KosmosIpcSplitAxis axis;
    gboolean new_pane_first;
} DetachedTabSplit;

typedef struct {
    guint64 workspace_id;
    guint64 pane_id;
    guint64 tab_id;
} PendingTabActivation;

typedef struct {
    KosmosMainWindow *window;
    AdwTabView *view;
} TabBarActivation;

typedef struct {
    KosmosMainWindow *window;
    AdwTabView *view;
} DeferredTabActivation;

typedef struct {
    guint64 target_pane_id;
    KosmosIpcSplitAxis axis;
    gboolean new_pane_first;
} SplitDropZone;

typedef struct {
    KosmosMainWindow *window;
    AdwTabView *view;
    GtkWidget *source;
    GtkWidget *excluded_widget;
    GtkWidget *preview;
    char *title;
    guint64 workspace_id;
    guint64 pane_id;
    double start_x;
    double start_y;
    gboolean active;
} PaneDrag;

typedef struct {
    GtkOrientation orientation;
    double ratio;
} PanedRatio;

static const int KOSMOS_MIN_PANE_WIDTH = 220;
static const int KOSMOS_MIN_PANE_HEIGHT = 160;

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

static void install_split_drop_css(GtkWidget *widget) {
    static gboolean installed = FALSE;

    if (installed) {
        return;
    }

    GtkCssProvider *provider = gtk_css_provider_new();
    gtk_css_provider_load_from_string(
        provider,
        ".split-drop-highlight {"
        "  background: rgba(53, 132, 228, 0.24);"
        "  border: 2px solid rgba(53, 132, 228, 0.72);"
        "  border-radius: 10px;"
        "}"
        ".pane-drag-preview {"
        "  background: rgba(98, 99, 107, 0.96);"
        "  border-radius: 9px;"
        "  padding: 8px 12px;"
        "  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.32);"
        "}"
    );
    gtk_style_context_add_provider_for_display(
        gtk_widget_get_display(widget),
        GTK_STYLE_PROVIDER(provider),
        GTK_STYLE_PROVIDER_PRIORITY_APPLICATION
    );
    g_object_unref(provider);
    installed = TRUE;
}

static void clear_content_area(KosmosMainWindow *self) {
    self->suppress_tab_detach = TRUE;

    if (self->pane_views != NULL) {
        g_hash_table_remove_all(self->pane_views);
    }
    g_clear_pointer(&self->layout_signature, g_free);

    GtkWidget *child = gtk_widget_get_first_child(self->content_area);

    while (child != NULL) {
        GtkWidget *next = gtk_widget_get_next_sibling(child);
        gtk_box_remove(GTK_BOX(self->content_area), child);
        child = next;
    }

    self->suppress_tab_detach = FALSE;
}

static void set_uint64_data(GObject *object, const char *key, guint64 value) {
    guint64 *stored_value = g_new(guint64, 1);
    *stored_value = value;
    g_object_set_data_full(object, key, stored_value, g_free);
}

static gboolean get_uint64_data(GObject *object, const char *key, guint64 *value) {
    guint64 *stored_value = g_object_get_data(object, key);

    if (stored_value == NULL) {
        return FALSE;
    }

    *value = *stored_value;
    return TRUE;
}

static void register_pane_view(KosmosMainWindow *self, guint64 pane_id, AdwTabView *view) {
    guint64 *key = g_new(guint64, 1);
    *key = pane_id;
    g_hash_table_replace(self->pane_views, key, view);
}

static AdwTabView *pane_view_for(KosmosMainWindow *self, guint64 pane_id) {
    return g_hash_table_lookup(self->pane_views, &pane_id);
}

static void set_paned_ratio(KosmosMainWindow *self, const char *path, double ratio) {
    double *stored_ratio = g_new(double, 1);
    *stored_ratio = CLAMP(ratio, 0.05, 0.95);
    g_hash_table_replace(self->paned_ratios, g_strdup(path), stored_ratio);
}

static gboolean get_paned_ratio(KosmosMainWindow *self, const char *path, double *ratio) {
    double *stored_ratio = g_hash_table_lookup(self->paned_ratios, path);

    if (stored_ratio == NULL) {
        return FALSE;
    }

    *ratio = *stored_ratio;
    return TRUE;
}

static void remember_paned_ratios_from_widget(KosmosMainWindow *self, GtkWidget *widget) {
    if (GTK_IS_PANED(widget)) {
        const char *path = g_object_get_data(G_OBJECT(widget), "pane-path");
        GtkOrientation orientation = gtk_orientable_get_orientation(GTK_ORIENTABLE(widget));
        int size = orientation == GTK_ORIENTATION_HORIZONTAL
            ? gtk_widget_get_width(widget)
            : gtk_widget_get_height(widget);

        if (path != NULL && size > 1) {
            set_paned_ratio(self, path, (double)gtk_paned_get_position(GTK_PANED(widget)) / size);
        }
    }

    for (GtkWidget *child = gtk_widget_get_first_child(widget); child != NULL; child = gtk_widget_get_next_sibling(child)) {
        remember_paned_ratios_from_widget(self, child);
    }
}

static void remember_paned_ratios(KosmosMainWindow *self) {
    if (self->content_area == NULL) {
        return;
    }

    remember_paned_ratios_from_widget(self, self->content_area);
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

static gboolean get_double_member(JsonObject *object, const char *name, double *value) {
    JsonNode *node = json_object_get_member(object, name);
    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node)) {
        return FALSE;
    }

    *value = json_node_get_double(node);
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
    clear_content_area(self);

    if (status == NULL || status[0] == '\0') {
        return;
    }

    GtkWidget *label = create_label(status, "dim-label");
    gtk_widget_set_halign(label, GTK_ALIGN_CENTER);
    gtk_widget_set_valign(label, GTK_ALIGN_CENTER);
    gtk_widget_set_vexpand(label, TRUE);
    gtk_box_append(GTK_BOX(self->content_area), label);
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
        if (get_uint_member(workspace, "id", &workspace_id) && workspace_id == active_workspace_id) {
            return workspace;
        }
    }

    return NULL;
}

static gboolean append_layout_signature(GString *signature, JsonObject *node) {
    const char *type = NULL;
    if (!get_string_member(node, "type", &type)) {
        return FALSE;
    }

    if (g_strcmp0(type, "leaf") == 0) {
        JsonObject *pane = get_object_member(node, "pane");
        guint64 pane_id = 0;
        if (pane == NULL || !get_uint_member(pane, "id", &pane_id)) {
            return FALSE;
        }

        g_string_append_printf(signature, "L:%" G_GUINT64_FORMAT, pane_id);
        return TRUE;
    }

    if (g_strcmp0(type, "split") == 0) {
        const char *axis = NULL;
        JsonObject *first = get_object_member(node, "first");
        JsonObject *second = get_object_member(node, "second");
        if (!get_string_member(node, "axis", &axis) || first == NULL || second == NULL) {
            return FALSE;
        }

        g_string_append_printf(signature, "S:%s(", axis);
        if (!append_layout_signature(signature, first)) {
            return FALSE;
        }
        g_string_append_c(signature, ',');
        if (!append_layout_signature(signature, second)) {
            return FALSE;
        }
        g_string_append_c(signature, ')');
        return TRUE;
    }

    return FALSE;
}

static char *create_layout_signature(JsonObject *workspace) {
    JsonObject *root = get_object_member(workspace, "root");
    if (root == NULL) {
        return NULL;
    }

    GString *signature = g_string_new(NULL);
    if (!append_layout_signature(signature, root)) {
        g_string_free(signature, TRUE);
        return NULL;
    }

    return g_string_free(signature, FALSE);
}

static void collect_split_paths(JsonObject *node, const char *path, GHashTable *split_paths) {
    const char *type = NULL;
    if (!get_string_member(node, "type", &type)) {
        return;
    }

    if (g_strcmp0(type, "split") != 0) {
        return;
    }

    JsonObject *first = get_object_member(node, "first");
    JsonObject *second = get_object_member(node, "second");
    if (first == NULL || second == NULL) {
        return;
    }

    g_hash_table_add(split_paths, g_strdup(path));

    char *first_path = g_strconcat(path, "/first", NULL);
    char *second_path = g_strconcat(path, "/second", NULL);
    collect_split_paths(first, first_path, split_paths);
    collect_split_paths(second, second_path, split_paths);
    g_free(first_path);
    g_free(second_path);
}

static void prune_paned_ratios_for_root(KosmosMainWindow *self, JsonObject *root) {
    GHashTable *split_paths = g_hash_table_new_full(g_str_hash, g_str_equal, g_free, NULL);
    collect_split_paths(root, "root", split_paths);

    GHashTableIter iter;
    gpointer key = NULL;
    gpointer value = NULL;
    g_hash_table_iter_init(&iter, self->paned_ratios);
    while (g_hash_table_iter_next(&iter, &key, &value)) {
        if (!g_hash_table_contains(split_paths, key)) {
            g_hash_table_iter_remove(&iter);
        }
    }

    g_hash_table_unref(split_paths);
}

static void apply_server_state_or_show_error(KosmosMainWindow *self, JsonNode *state, GError *error, const char *error_prefix) {
    if (error != NULL) {
        set_error_status(self, error_prefix, error);
        return;
    }

    apply_workspace_state(self, state);
}

static void clear_pending_tab_activation(AdwTabView *view) {
    g_object_set_data(G_OBJECT(view), "pending-tab-activation", NULL);
}

static void set_pending_tab_activation(AdwTabView *view, guint64 workspace_id, guint64 pane_id, guint64 tab_id) {
    PendingTabActivation *activation = g_new(PendingTabActivation, 1);
    activation->workspace_id = workspace_id;
    activation->pane_id = pane_id;
    activation->tab_id = tab_id;

    g_object_set_data_full(G_OBJECT(view), "pending-tab-activation", activation, g_free);
}

static void request_tab_activation(KosmosMainWindow *self, guint64 workspace_id, guint64 pane_id, guint64 tab_id) {
    if (!ensure_connected(self)) {
        return;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    kosmos_ipc_client_activate_tab(self->ipc_client, workspace_id, pane_id, tab_id, &state, NULL, &error);
    apply_server_state_or_show_error(self, state, error, "Failed to activate tab");

    g_clear_error(&error);
    if (state != NULL) {
        json_node_unref(state);
    }
}

static void tab_bar_pressed(GtkGestureClick *gesture, int n_press, double x, double y, gpointer user_data) {
    (void)gesture;
    (void)n_press;
    (void)x;
    (void)y;

    TabBarActivation *activation = user_data;
    g_object_set_data(G_OBJECT(activation->view), "tab-press-active", GINT_TO_POINTER(1));
}

static void deferred_tab_activation_free(DeferredTabActivation *activation) {
    g_object_unref(activation->window);
    g_object_unref(activation->view);
    g_free(activation);
}

static gboolean finish_pending_tab_activation(gpointer user_data) {
    DeferredTabActivation *activation = user_data;
    AdwTabView *view = activation->view;
    PendingTabActivation *pending = g_object_get_data(G_OBJECT(view), "pending-tab-activation");
    PendingTabActivation pending_copy = {0};

    if (gtk_widget_get_root(GTK_WIDGET(view)) == NULL || pending == NULL || adw_tab_view_get_is_transferring_page(view)) {
        clear_pending_tab_activation(view);
        return G_SOURCE_REMOVE;
    }

    pending_copy = *pending;
    clear_pending_tab_activation(view);
    request_tab_activation(activation->window, pending_copy.workspace_id, pending_copy.pane_id, pending_copy.tab_id);

    return G_SOURCE_REMOVE;
}

static void tab_bar_released(GtkGestureClick *gesture, int n_press, double x, double y, gpointer user_data) {
    (void)gesture;
    (void)n_press;
    (void)x;
    (void)y;

    TabBarActivation *activation = user_data;
    AdwTabView *view = activation->view;
    PendingTabActivation *pending = g_object_get_data(G_OBJECT(view), "pending-tab-activation");

    g_object_set_data(G_OBJECT(view), "tab-press-active", NULL);
    if (pending == NULL || adw_tab_view_get_is_transferring_page(view)) {
        clear_pending_tab_activation(view);
        return;
    }

    DeferredTabActivation *deferred = g_new(DeferredTabActivation, 1);
    deferred->window = g_object_ref(activation->window);
    deferred->view = g_object_ref(view);
    g_idle_add_full(G_PRIORITY_DEFAULT_IDLE, finish_pending_tab_activation, deferred, (GDestroyNotify)deferred_tab_activation_free);
}

static void tab_transfer_changed(GObject *object, GParamSpec *pspec, gpointer user_data) {
    (void)pspec;
    (void)user_data;

    AdwTabView *view = ADW_TAB_VIEW(object);
    if (adw_tab_view_get_is_transferring_page(view)) {
        clear_pending_tab_activation(view);
    }
}

static void tab_detached(AdwTabView *view, AdwTabPage *page, int position, gpointer user_data) {
    (void)page;
    (void)position;
    (void)user_data;

    clear_pending_tab_activation(view);
}

static void activate_selected_tab(GObject *object, GParamSpec *pspec, gpointer user_data) {
    (void)pspec;

    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    if (self->applying_server_state) {
        return;
    }

    AdwTabView *view = ADW_TAB_VIEW(object);
    if (adw_tab_view_get_is_transferring_page(view)) {
        return;
    }

    AdwTabPage *page = adw_tab_view_get_selected_page(view);

    if (page == NULL || !ensure_connected(self)) {
        return;
    }

    guint64 workspace_id = 0;
    guint64 pane_id = 0;
    guint64 tab_id = 0;
    if (!get_uint64_data(G_OBJECT(page), "workspace-id", &workspace_id) ||
        !get_uint64_data(G_OBJECT(page), "pane-id", &pane_id) ||
        !get_uint64_data(G_OBJECT(page), "tab-id", &tab_id)) {
        return;
    }

    if (g_object_get_data(G_OBJECT(view), "tab-press-active") != NULL) {
        set_pending_tab_activation(view, workspace_id, pane_id, tab_id);
        return;
    }

    request_tab_activation(self, workspace_id, pane_id, tab_id);
}

static gboolean close_tab(AdwTabView *view, AdwTabPage *page, gpointer user_data) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);

    if (self->applying_server_state) {
        adw_tab_view_close_page_finish(view, page, TRUE);
        return GDK_EVENT_STOP;
    }

    guint64 workspace_id = 0;
    guint64 pane_id = 0;
    guint64 tab_id = 0;
    if (!get_uint64_data(G_OBJECT(page), "workspace-id", &workspace_id) ||
        !get_uint64_data(G_OBJECT(page), "pane-id", &pane_id) ||
        !get_uint64_data(G_OBJECT(page), "tab-id", &tab_id) ||
        !ensure_connected(self)) {
        adw_tab_view_close_page_finish(view, page, FALSE);
        return GDK_EVENT_STOP;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    gboolean closed = kosmos_ipc_client_close_tab(self->ipc_client, workspace_id, pane_id, tab_id, &state, NULL, &error);
    adw_tab_view_close_page_finish(view, page, closed);
    apply_server_state_or_show_error(self, state, error, "Failed to close tab");

    g_clear_error(&error);
    if (state != NULL) {
        json_node_unref(state);
    }

    return GDK_EVENT_STOP;
}

static void reorder_tab(AdwTabView *view, AdwTabPage *page, int position, gpointer user_data) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    clear_pending_tab_activation(view);

    if (self->applying_server_state) {
        return;
    }

    if (adw_tab_view_get_is_transferring_page(view)) {
        return;
    }

    guint64 workspace_id = 0;
    guint64 pane_id = 0;
    guint64 tab_id = 0;
    if (!get_uint64_data(G_OBJECT(page), "workspace-id", &workspace_id) ||
        !get_uint64_data(G_OBJECT(page), "pane-id", &pane_id) ||
        !get_uint64_data(G_OBJECT(page), "tab-id", &tab_id) ||
        !ensure_connected(self)) {
        kosmos_main_window_refresh_workspace_state(self);
        return;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    kosmos_ipc_client_reorder_tab(self->ipc_client, workspace_id, pane_id, tab_id, (guint)position, &state, NULL, &error);

    if (error != NULL) {
        g_clear_error(&error);
        kosmos_main_window_refresh_workspace_state(self);
        return;
    }

    apply_workspace_state(self, state);
    if (state != NULL) {
        json_node_unref(state);
    }
}

static void detached_tab_split_free(DetachedTabSplit *split) {
    g_object_unref(split->window);
    g_free(split);
}

static void clear_detached_tab_transfer(KosmosMainWindow *self) {
    if (self->detached_tab_window != NULL) {
        gtk_window_destroy(self->detached_tab_window);
    }

    g_clear_object(&self->detached_tab_window);
    g_clear_object(&self->detached_tab_sink);
}

static void request_split_tab(
    KosmosMainWindow *self,
    guint64 workspace_id,
    guint64 pane_id,
    guint64 target_pane_id,
    guint64 tab_id,
    KosmosIpcSplitAxis axis,
    gboolean new_pane_first
) {
    if (!ensure_connected(self)) {
        return;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    kosmos_ipc_client_split_tab(
        self->ipc_client,
        workspace_id,
        pane_id,
        target_pane_id,
        tab_id,
        axis,
        new_pane_first,
        &state,
        NULL,
        &error
    );
    apply_server_state_or_show_error(self, state, error, "Failed to split tab");

    g_clear_error(&error);
    if (state != NULL) {
        json_node_unref(state);
    }
}

static void request_move_pane(
    KosmosMainWindow *self,
    guint64 workspace_id,
    guint64 pane_id,
    guint64 target_pane_id,
    KosmosIpcSplitAxis axis,
    gboolean new_pane_first
) {
    if (!ensure_connected(self)) {
        return;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    kosmos_ipc_client_move_pane(
        self->ipc_client,
        workspace_id,
        pane_id,
        target_pane_id,
        axis,
        new_pane_first,
        &state,
        NULL,
        &error
    );
    apply_server_state_or_show_error(self, state, error, "Failed to move pane");

    g_clear_error(&error);
    if (state != NULL) {
        json_node_unref(state);
    }
}

static gboolean finish_detached_tab_split(gpointer user_data) {
    DetachedTabSplit *split = user_data;
    KosmosMainWindow *self = split->window;

    request_split_tab(
        self,
        split->workspace_id,
        split->pane_id,
        split->target_pane_id,
        split->tab_id,
        split->axis,
        split->new_pane_first
    );

    clear_detached_tab_transfer(self);
    return G_SOURCE_REMOVE;
}

static void split_attached_tab(AdwTabView *view, AdwTabPage *page, int position, gpointer user_data) {
    (void)position;

    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    gboolean is_split_target = g_object_get_data(G_OBJECT(view), "split-drop-target") != NULL;

    if (!self->splitting_detached_tab && !is_split_target) {
        return;
    }

    self->splitting_detached_tab = FALSE;

    guint64 workspace_id = 0;
    guint64 pane_id = 0;
    guint64 tab_id = 0;
    if (!get_uint64_data(G_OBJECT(page), "workspace-id", &workspace_id) ||
        !get_uint64_data(G_OBJECT(page), "pane-id", &pane_id) ||
        !get_uint64_data(G_OBJECT(page), "tab-id", &tab_id)) {
        kosmos_main_window_refresh_workspace_state(self);
        return;
    }

    guint64 target_pane_id = pane_id;
    get_uint64_data(G_OBJECT(view), "target-pane-id", &target_pane_id);

    gpointer axis_data = g_object_get_data(G_OBJECT(view), "split-axis");
    KosmosIpcSplitAxis axis = axis_data == NULL
        ? KOSMOS_IPC_SPLIT_AXIS_HORIZONTAL
        : (KosmosIpcSplitAxis)(GPOINTER_TO_INT(axis_data) - 1);

    gpointer new_pane_first_data = g_object_get_data(G_OBJECT(view), "split-new-pane-first");
    gboolean new_pane_first = new_pane_first_data == NULL
        ? FALSE
        : GPOINTER_TO_INT(new_pane_first_data) - 1;

    DetachedTabSplit *split = g_new(DetachedTabSplit, 1);
    split->window = g_object_ref(self);
    split->workspace_id = workspace_id;
    split->pane_id = pane_id;
    split->target_pane_id = target_pane_id;
    split->tab_id = tab_id;
    split->axis = axis;
    split->new_pane_first = new_pane_first;
    g_idle_add_full(G_PRIORITY_DEFAULT_IDLE, finish_detached_tab_split, split, (GDestroyNotify)detached_tab_split_free);
}

static AdwTabView *create_split_sink_for_detached_tab(AdwTabView *view, gpointer user_data) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    clear_pending_tab_activation(view);
    clear_detached_tab_transfer(self);

    GtkWidget *window = adw_window_new();
    AdwTabView *sink = adw_tab_view_new();

    self->splitting_detached_tab = TRUE;
    self->detached_tab_window = GTK_WINDOW(g_object_ref_sink(window));
    self->detached_tab_sink = ADW_TAB_VIEW(g_object_ref_sink(sink));

    g_object_set_data(G_OBJECT(sink), "split-drop-target", GINT_TO_POINTER(1));
    gtk_window_set_child(self->detached_tab_window, GTK_WIDGET(sink));
    gtk_window_set_transient_for(self->detached_tab_window, GTK_WINDOW(self));
    g_signal_connect(sink, "page-attached", G_CALLBACK(split_attached_tab), self);
    gtk_window_present(self->detached_tab_window);

    return sink;
}

static void open_blank_tab(GtkButton *button, gpointer user_data) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);

    guint64 workspace_id = 0;
    guint64 pane_id = 0;
    if (!get_uint64_data(G_OBJECT(button), "workspace-id", &workspace_id) ||
        !get_uint64_data(G_OBJECT(button), "pane-id", &pane_id) ||
        !ensure_connected(self)) {
        return;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    kosmos_ipc_client_open_tab(
        self->ipc_client,
        workspace_id,
        pane_id,
        "Blank",
        KOSMOS_IPC_TAB_KIND_BLANK,
        &state,
        NULL,
        &error
    );
    apply_server_state_or_show_error(self, state, error, "Failed to open tab");

    g_clear_error(&error);
    if (state != NULL) {
        json_node_unref(state);
    }
}

static GtkWidget *create_new_tab_button(guint64 workspace_id, guint64 pane_id) {
    GtkWidget *button = gtk_button_new_from_icon_name("list-add-symbolic");
    gtk_widget_set_tooltip_text(button, "Open blank tab");
    set_uint64_data(G_OBJECT(button), "workspace-id", workspace_id);
    set_uint64_data(G_OBJECT(button), "pane-id", pane_id);

    return button;
}

static GtkWidget *create_tab_content(const char *kind) {
    (void)kind;

    GtkWidget *content = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    gtk_widget_set_hexpand(content, TRUE);
    gtk_widget_set_vexpand(content, TRUE);
    return content;
}

static void configure_tab_page(AdwTabPage *page, guint64 workspace_id, guint64 pane_id, guint64 tab_id, const char *title) {
    adw_tab_page_set_title(page, title);
    adw_tab_page_set_tooltip(page, title);
    set_uint64_data(G_OBJECT(page), "workspace-id", workspace_id);
    set_uint64_data(G_OBJECT(page), "pane-id", pane_id);
    set_uint64_data(G_OBJECT(page), "tab-id", tab_id);
}

static void add_tab_bar_activation_controller(KosmosMainWindow *self, GtkWidget *tab_bar, AdwTabView *tab_view) {
    GtkGesture *gesture = gtk_gesture_click_new();
    TabBarActivation *activation = g_new(TabBarActivation, 1);

    activation->window = self;
    activation->view = tab_view;

    gtk_gesture_single_set_button(GTK_GESTURE_SINGLE(gesture), GDK_BUTTON_PRIMARY);
    gtk_event_controller_set_propagation_phase(GTK_EVENT_CONTROLLER(gesture), GTK_PHASE_CAPTURE);
    g_object_set_data_full(G_OBJECT(gesture), "tab-bar-activation", activation, g_free);
    g_signal_connect(gesture, "pressed", G_CALLBACK(tab_bar_pressed), activation);
    g_signal_connect(gesture, "released", G_CALLBACK(tab_bar_released), activation);
    gtk_widget_add_controller(tab_bar, GTK_EVENT_CONTROLLER(gesture));
}

static void set_split_drop_highlight(GtkDropControllerMotion *controller, gboolean highlighted) {
    GtkWidget *highlight = g_object_get_data(G_OBJECT(controller), "split-drop-highlight");

    if (highlight != NULL) {
        gtk_widget_set_opacity(highlight, highlighted ? 1.0 : 0.0);
    }
}

static void set_split_zone_highlight(GtkWidget *zone, gboolean highlighted) {
    GtkWidget *highlight = g_object_get_data(G_OBJECT(zone), "split-drop-highlight");

    if (highlight != NULL) {
        gtk_widget_set_opacity(highlight, highlighted ? 1.0 : 0.0);
    }
}

static void set_pane_drag_highlight(KosmosMainWindow *self, GtkWidget *zone) {
    GtkWidget *current_zone = g_object_get_data(G_OBJECT(self), "pane-drag-highlight-zone");

    if (current_zone == zone) {
        return;
    }

    if (current_zone != NULL) {
        set_split_zone_highlight(current_zone, FALSE);
    }

    if (zone != NULL) {
        set_split_zone_highlight(zone, TRUE);
    }

    g_object_set_data(G_OBJECT(self), "pane-drag-highlight-zone", zone);
}

static gboolean split_zone_contains_point(GtkWidget *zone, GtkWidget *source, double source_x, double source_y) {
    graphene_point_t source_point = GRAPHENE_POINT_INIT((float)source_x, (float)source_y);
    graphene_point_t zone_point = GRAPHENE_POINT_INIT(0.0f, 0.0f);

    if (!gtk_widget_compute_point(source, zone, &source_point, &zone_point)) {
        return FALSE;
    }

    return zone_point.x >= 0.0f &&
        zone_point.y >= 0.0f &&
        zone_point.x < gtk_widget_get_width(zone) &&
        zone_point.y < gtk_widget_get_height(zone);
}

static gboolean find_split_zone_at(
    GtkWidget *widget,
    GtkWidget *source,
    double source_x,
    double source_y,
    guint64 source_pane_id,
    GtkWidget **zone_widget,
    SplitDropZone *zone
) {
    if (!gtk_widget_get_visible(widget)) {
        return FALSE;
    }

    SplitDropZone *candidate = g_object_get_data(G_OBJECT(widget), "split-drop-zone");
    if (candidate != NULL &&
        candidate->target_pane_id != source_pane_id &&
        split_zone_contains_point(widget, source, source_x, source_y)) {
        *zone_widget = widget;
        *zone = *candidate;
        return TRUE;
    }

    for (GtkWidget *child = gtk_widget_get_first_child(widget); child != NULL; child = gtk_widget_get_next_sibling(child)) {
        if (find_split_zone_at(child, source, source_x, source_y, source_pane_id, zone_widget, zone)) {
            return TRUE;
        }
    }

    return FALSE;
}

static void show_pane_drag_preview(PaneDrag *drag);
static gboolean pane_drag_start_is_valid(PaneDrag *drag, double x, double y);

static void pane_drag_begin(GtkGestureDrag *gesture, double start_x, double start_y, gpointer user_data) {
    PaneDrag *drag = user_data;

    drag->active = FALSE;
    if (!pane_drag_start_is_valid(drag, start_x, start_y)) {
        gtk_gesture_set_state(GTK_GESTURE(gesture), GTK_EVENT_SEQUENCE_DENIED);
        return;
    }

    drag->start_x = start_x;
    drag->start_y = start_y;
    set_pane_drag_highlight(drag->window, NULL);
}

static gboolean pane_drag_find_zone(PaneDrag *drag, double x, double y, GtkWidget **zone_widget, SplitDropZone *zone) {
    return find_split_zone_at(
        drag->window->content_area,
        drag->source,
        x,
        y,
        drag->pane_id,
        zone_widget,
        zone
    );
}

static gboolean picked_widget_is_or_contains(GtkWidget *picked, GtkWidget *widget) {
    if (widget == NULL || picked == NULL) {
        return FALSE;
    }

    return picked == widget || (picked != NULL && gtk_widget_is_ancestor(picked, widget));
}

static gboolean pane_drag_starts_on_action_area(PaneDrag *drag, double x, double y) {
    if (drag->excluded_widget == NULL) {
        return FALSE;
    }

    graphene_point_t action_origin = GRAPHENE_POINT_INIT(0.0f, 0.0f);
    graphene_point_t source_point = GRAPHENE_POINT_INIT(0.0f, 0.0f);
    if (gtk_widget_compute_point(drag->excluded_widget, drag->source, &action_origin, &source_point) && x >= source_point.x - 8.0) {
        return TRUE;
    }

    return split_zone_contains_point(drag->excluded_widget, drag->source, x, y);
}

static gboolean pane_drag_starts_in_tab_region(PaneDrag *drag, double x, double y) {
    if (x < 0.0 || y < 0.0 || x >= gtk_widget_get_width(drag->source) || y >= gtk_widget_get_height(drag->source)) {
        return FALSE;
    }

    double action_start = gtk_widget_get_width(drag->source);
    if (drag->excluded_widget != NULL) {
        graphene_point_t action_origin = GRAPHENE_POINT_INIT(0.0f, 0.0f);
        graphene_point_t source_point = GRAPHENE_POINT_INIT(0.0f, 0.0f);
        if (gtk_widget_compute_point(drag->excluded_widget, drag->source, &action_origin, &source_point)) {
            action_start = source_point.x;
        }
    }

    double tab_region_end = MIN(action_start - 8.0, 260.0);
    return x <= tab_region_end;
}

static gboolean pane_drag_start_is_valid(PaneDrag *drag, double x, double y) {
    GtkWidget *picked = gtk_widget_pick(drag->source, x, y, GTK_PICK_DEFAULT);

    if (picked == NULL || picked == drag->source) {
        return FALSE;
    }

    if (picked_widget_is_or_contains(picked, drag->excluded_widget) || pane_drag_starts_on_action_area(drag, x, y)) {
        return FALSE;
    }

    return pane_drag_starts_in_tab_region(drag, x, y);
}

static gboolean pane_drag_offset_is_significant(double offset_x, double offset_y) {
    return offset_x * offset_x + offset_y * offset_y >= 64.0;
}

static GtkWidget *create_pane_drag_preview(const char *title) {
    GtkWidget *preview = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 10);
    GtkWidget *label = gtk_label_new(title == NULL || title[0] == '\0' ? "Blank" : title);
    GtkWidget *close_icon = gtk_image_new_from_icon_name("window-close-symbolic");

    gtk_widget_add_css_class(preview, "pane-drag-preview");
    gtk_widget_set_can_target(preview, FALSE);
    gtk_widget_set_halign(preview, GTK_ALIGN_START);
    gtk_widget_set_valign(preview, GTK_ALIGN_START);
    gtk_widget_set_size_request(preview, 170, -1);
    gtk_label_set_xalign(GTK_LABEL(label), 0.5f);
    gtk_label_set_ellipsize(GTK_LABEL(label), PANGO_ELLIPSIZE_END);
    gtk_widget_set_hexpand(label, TRUE);
    gtk_widget_set_can_target(label, FALSE);
    gtk_widget_set_can_target(close_icon, FALSE);

    gtk_box_append(GTK_BOX(preview), label);
    gtk_box_append(GTK_BOX(preview), close_icon);

    return preview;
}

static void set_pane_drag_preview_position(PaneDrag *drag, double x, double y) {
    if (drag->preview == NULL) {
        return;
    }

    graphene_point_t source_point = GRAPHENE_POINT_INIT((float)x, (float)y);
    graphene_point_t overlay_point = GRAPHENE_POINT_INIT(0.0f, 0.0f);
    if (!gtk_widget_compute_point(drag->source, drag->window->content_overlay, &source_point, &overlay_point)) {
        return;
    }

    gtk_widget_set_margin_start(drag->preview, MAX(0, (int)overlay_point.x - 85));
    gtk_widget_set_margin_top(drag->preview, MAX(0, (int)overlay_point.y - 18));
}

static void show_pane_drag_preview(PaneDrag *drag) {
    if (drag->preview != NULL) {
        return;
    }

    drag->preview = create_pane_drag_preview(drag->title);
    gtk_overlay_add_overlay(GTK_OVERLAY(drag->window->content_overlay), drag->preview);
    set_pane_drag_preview_position(drag, drag->start_x, drag->start_y);
}

static void clear_pane_drag_preview(PaneDrag *drag) {
    if (drag->preview == NULL) {
        return;
    }

    gtk_overlay_remove_overlay(GTK_OVERLAY(drag->window->content_overlay), drag->preview);
    drag->preview = NULL;
}

static void pane_drag_update(GtkGestureDrag *gesture, double offset_x, double offset_y, gpointer user_data) {
    (void)gesture;

    PaneDrag *drag = user_data;
    GtkWidget *zone_widget = NULL;
    SplitDropZone zone = {0};
    double x = drag->start_x + offset_x;
    double y = drag->start_y + offset_y;

    if (!drag->active) {
        if (!pane_drag_offset_is_significant(offset_x, offset_y)) {
            return;
        }

        drag->active = TRUE;
        clear_pending_tab_activation(drag->view);
        g_object_set_data(G_OBJECT(drag->view), "tab-press-active", NULL);
        show_pane_drag_preview(drag);
        gtk_gesture_set_state(GTK_GESTURE(gesture), GTK_EVENT_SEQUENCE_CLAIMED);
    }

    set_pane_drag_preview_position(drag, x, y);
    if (pane_drag_find_zone(drag, x, y, &zone_widget, &zone)) {
        set_pane_drag_highlight(drag->window, zone_widget);
    } else {
        set_pane_drag_highlight(drag->window, NULL);
    }
}

static void pane_drag_end(GtkGestureDrag *gesture, double offset_x, double offset_y, gpointer user_data) {
    (void)gesture;

    PaneDrag *drag = user_data;
    GtkWidget *zone_widget = NULL;
    SplitDropZone zone = {0};
    gboolean has_zone = pane_drag_find_zone(drag, drag->start_x + offset_x, drag->start_y + offset_y, &zone_widget, &zone);

    if (!drag->active) {
        return;
    }

    drag->active = FALSE;
    set_pane_drag_highlight(drag->window, NULL);
    clear_pane_drag_preview(drag);
    if (!has_zone) {
        return;
    }

    request_move_pane(
        drag->window,
        drag->workspace_id,
        drag->pane_id,
        zone.target_pane_id,
        zone.axis,
        zone.new_pane_first
    );
}

static void pane_drag_free(PaneDrag *drag) {
    clear_pane_drag_preview(drag);
    g_free(drag->title);
    g_free(drag);
}

static void remove_single_tab_pane_drag(GtkWidget *widget) {
    GtkEventController *controller = g_object_get_data(G_OBJECT(widget), "pane-drag-controller");

    if (controller == NULL) {
        return;
    }

    g_object_set_data(G_OBJECT(widget), "pane-drag-controller", NULL);
    gtk_widget_remove_controller(widget, controller);
}

static void add_single_tab_pane_drag(
    KosmosMainWindow *self,
    GtkWidget *widget,
    GtkWidget *excluded_widget,
    AdwTabView *tab_view,
    guint64 workspace_id,
    guint64 pane_id,
    const char *title
) {
    GtkGesture *gesture = gtk_gesture_drag_new();
    PaneDrag *drag = g_new0(PaneDrag, 1);

    remove_single_tab_pane_drag(widget);

    drag->window = self;
    drag->view = tab_view;
    drag->source = widget;
    drag->excluded_widget = excluded_widget;
    drag->title = g_strdup(title);
    drag->workspace_id = workspace_id;
    drag->pane_id = pane_id;

    gtk_gesture_single_set_button(GTK_GESTURE_SINGLE(gesture), GDK_BUTTON_PRIMARY);
    gtk_event_controller_set_propagation_phase(GTK_EVENT_CONTROLLER(gesture), GTK_PHASE_CAPTURE);
    g_object_set_data_full(G_OBJECT(gesture), "pane-drag", drag, (GDestroyNotify)pane_drag_free);
    g_signal_connect(gesture, "drag-begin", G_CALLBACK(pane_drag_begin), drag);
    g_signal_connect(gesture, "drag-update", G_CALLBACK(pane_drag_update), drag);
    g_signal_connect(gesture, "drag-end", G_CALLBACK(pane_drag_end), drag);
    gtk_widget_add_controller(widget, GTK_EVENT_CONTROLLER(gesture));
    g_object_set_data(G_OBJECT(widget), "pane-drag-controller", gesture);
}

static void configure_single_tab_pane_drag(
    KosmosMainWindow *self,
    GtkWidget *tab_bar,
    GtkWidget *new_tab_button,
    AdwTabView *tab_view,
    guint64 workspace_id,
    guint64 pane_id,
    int tab_count,
    AdwTabPage *active_page
) {
    if (tab_count == 1 && active_page != NULL) {
        add_single_tab_pane_drag(
            self,
            tab_bar,
            new_tab_button,
            tab_view,
            workspace_id,
            pane_id,
            adw_tab_page_get_title(active_page)
        );
        return;
    }

    remove_single_tab_pane_drag(tab_bar);
}

static AdwTabPage *tab_page_for_id(AdwTabView *view, guint64 tab_id) {
    int page_count = adw_tab_view_get_n_pages(view);

    for (int index = 0; index < page_count; index++) {
        AdwTabPage *page = adw_tab_view_get_nth_page(view, index);
        guint64 page_tab_id = 0;
        if (get_uint64_data(G_OBJECT(page), "tab-id", &page_tab_id) && page_tab_id == tab_id) {
            return page;
        }
    }

    return NULL;
}

static JsonObject *get_tab_snapshot(JsonArray *tabs, guint index) {
    JsonNode *tab_node = json_array_get_element(tabs, index);

    if (tab_node == NULL || !JSON_NODE_HOLDS_OBJECT(tab_node)) {
        return NULL;
    }

    return json_node_get_object(tab_node);
}

static gboolean tab_view_matches_snapshot_prefix(AdwTabView *view, JsonArray *tabs) {
    int page_count = adw_tab_view_get_n_pages(view);

    if (json_array_get_length(tabs) < (guint)page_count) {
        return FALSE;
    }

    for (int index = 0; index < page_count; index++) {
        AdwTabPage *page = adw_tab_view_get_nth_page(view, index);
        JsonObject *tab = get_tab_snapshot(tabs, (guint)index);
        guint64 page_tab_id = 0;
        guint64 snapshot_tab_id = 0;

        if (tab == NULL ||
            !get_uint64_data(G_OBJECT(page), "tab-id", &page_tab_id) ||
            !get_uint_member(tab, "id", &snapshot_tab_id) ||
            page_tab_id != snapshot_tab_id) {
            return FALSE;
        }
    }

    return TRUE;
}

static gboolean tab_view_matches_snapshot_exact(AdwTabView *view, JsonArray *tabs) {
    return json_array_get_length(tabs) == (guint)adw_tab_view_get_n_pages(view) &&
        tab_view_matches_snapshot_prefix(view, tabs);
}

static gboolean update_tab_view_from_pane_view(
    KosmosMainWindow *self,
    AdwTabView *view,
    JsonObject *pane,
    guint64 workspace_id,
    guint64 active_pane_id,
    gboolean allow_append
) {
    guint64 pane_id = 0;
    guint64 active_tab_id = 0;
    JsonArray *tabs = get_array_member(pane, "tabs");

    if (!get_uint_member(pane, "id", &pane_id) || !get_uint_member(pane, "activeTabId", &active_tab_id) || tabs == NULL) {
        return FALSE;
    }

    if (allow_append) {
        if (!tab_view_matches_snapshot_prefix(view, tabs)) {
            return FALSE;
        }
    } else if (!tab_view_matches_snapshot_exact(view, tabs)) {
        return FALSE;
    }

    gboolean was_applying = self->applying_server_state;
    self->applying_server_state = TRUE;

    AdwTabPage *active_page = NULL;
    guint tab_count = json_array_get_length(tabs);
    for (guint index = 0; index < tab_count; index++) {
        JsonObject *tab = get_tab_snapshot(tabs, index);
        guint64 tab_id = 0;
        const char *title = NULL;
        const char *kind = NULL;
        if (tab == NULL || !get_uint_member(tab, "id", &tab_id) || !get_string_member(tab, "title", &title)) {
            continue;
        }
        get_string_member(tab, "kind", &kind);

        AdwTabPage *page = tab_page_for_id(view, tab_id);
        if (page == NULL) {
            if (!allow_append) {
                self->applying_server_state = was_applying;
                return FALSE;
            }

            page = adw_tab_view_append(view, create_tab_content(kind));
        }

        configure_tab_page(page, workspace_id, pane_id, tab_id, title);
        if (tab_id == active_tab_id) {
            active_page = page;
        }
    }

    if (active_page != NULL && adw_tab_view_get_selected_page(view) != active_page) {
        adw_tab_view_set_selected_page(view, active_page);
    }

    (void)active_pane_id;

    GtkWidget *tab_bar = g_object_get_data(G_OBJECT(view), "tab-bar");
    GtkWidget *new_tab_button = g_object_get_data(G_OBJECT(view), "new-tab-button");
    if (tab_bar != NULL && new_tab_button != NULL) {
        configure_single_tab_pane_drag(
            self,
            tab_bar,
            new_tab_button,
            view,
            workspace_id,
            pane_id,
            (int)tab_count,
            active_page
        );
    }

    self->applying_server_state = was_applying;
    register_pane_view(self, pane_id, view);
    return TRUE;
}

static gboolean update_tab_view_from_pane(KosmosMainWindow *self, JsonObject *pane, guint64 workspace_id, guint64 active_pane_id) {
    guint64 pane_id = 0;
    if (!get_uint_member(pane, "id", &pane_id)) {
        return FALSE;
    }

    AdwTabView *view = pane_view_for(self, pane_id);
    if (view == NULL) {
        return FALSE;
    }

    return update_tab_view_from_pane_view(self, view, pane, workspace_id, active_pane_id, TRUE);
}

static gboolean update_pane_node_in_place(KosmosMainWindow *self, JsonObject *node, guint64 workspace_id, guint64 active_pane_id) {
    const char *type = NULL;
    if (!get_string_member(node, "type", &type)) {
        return FALSE;
    }

    if (g_strcmp0(type, "leaf") == 0) {
        JsonObject *pane = get_object_member(node, "pane");
        return pane != NULL && update_tab_view_from_pane(self, pane, workspace_id, active_pane_id);
    }

    if (g_strcmp0(type, "split") == 0) {
        JsonObject *first = get_object_member(node, "first");
        JsonObject *second = get_object_member(node, "second");
        return first != NULL &&
            second != NULL &&
            update_pane_node_in_place(self, first, workspace_id, active_pane_id) &&
            update_pane_node_in_place(self, second, workspace_id, active_pane_id);
    }

    return FALSE;
}

static gboolean update_active_workspace_in_place(KosmosMainWindow *self, JsonObject *workspace) {
    guint64 workspace_id = 0;
    guint64 active_pane_id = 0;
    JsonObject *root = get_object_member(workspace, "root");

    if (!get_uint_member(workspace, "id", &workspace_id) ||
        !get_uint_member(workspace, "activePaneId", &active_pane_id) ||
        root == NULL) {
        return FALSE;
    }

    return update_pane_node_in_place(self, root, workspace_id, active_pane_id);
}

static void show_split_drop_highlight(GtkDropControllerMotion *controller, double x, double y, gpointer user_data) {
    (void)x;
    (void)y;
    (void)user_data;

    set_split_drop_highlight(controller, TRUE);
}

static void hide_split_drop_highlight(GtkDropControllerMotion *controller, gpointer user_data) {
    (void)user_data;

    set_split_drop_highlight(controller, FALSE);
}

static GtkWidget *create_split_drop_highlight(void) {
    GtkWidget *highlight = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    gtk_widget_add_css_class(highlight, "split-drop-highlight");
    gtk_widget_set_can_target(highlight, FALSE);
    gtk_widget_set_hexpand(highlight, TRUE);
    gtk_widget_set_vexpand(highlight, TRUE);
    gtk_widget_set_opacity(highlight, 0.0);

    return highlight;
}

static GtkWidget *create_split_drop_target(
    KosmosMainWindow *self,
    guint64 target_pane_id,
    KosmosIpcSplitAxis axis,
    gboolean new_pane_first
) {
    GtkWidget *target = gtk_overlay_new();
    GtkWidget *drop_receiver = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    GtkWidget *highlight = create_split_drop_highlight();
    GtkEventController *motion = GTK_EVENT_CONTROLLER(gtk_drop_controller_motion_new());
    AdwTabBar *tab_bar = adw_tab_bar_new();
    AdwTabView *tab_view = adw_tab_view_new();
    SplitDropZone *zone = g_new(SplitDropZone, 1);

    adw_tab_bar_set_view(tab_bar, tab_view);
    gtk_widget_set_visible(GTK_WIDGET(tab_bar), FALSE);
    gtk_widget_set_hexpand(GTK_WIDGET(tab_view), TRUE);
    gtk_widget_set_vexpand(GTK_WIDGET(tab_view), TRUE);
    gtk_widget_set_opacity(GTK_WIDGET(tab_view), 0.01);
    gtk_box_append(GTK_BOX(drop_receiver), GTK_WIDGET(tab_bar));
    gtk_box_append(GTK_BOX(drop_receiver), GTK_WIDGET(tab_view));
    gtk_overlay_set_child(GTK_OVERLAY(target), drop_receiver);
    gtk_overlay_add_overlay(GTK_OVERLAY(target), highlight);

    g_object_set_data(G_OBJECT(tab_view), "split-drop-target", GINT_TO_POINTER(1));
    set_uint64_data(G_OBJECT(tab_view), "target-pane-id", target_pane_id);
    g_object_set_data(G_OBJECT(tab_view), "split-axis", GINT_TO_POINTER(axis + 1));
    g_object_set_data(G_OBJECT(tab_view), "split-new-pane-first", GINT_TO_POINTER((new_pane_first ? 1 : 0) + 1));
    g_signal_connect(tab_view, "page-attached", G_CALLBACK(split_attached_tab), self);

    zone->target_pane_id = target_pane_id;
    zone->axis = axis;
    zone->new_pane_first = new_pane_first;
    g_object_set_data_full(G_OBJECT(target), "split-drop-zone", zone, g_free);
    g_object_set_data(G_OBJECT(target), "split-drop-highlight", highlight);

    g_object_set_data(G_OBJECT(motion), "split-drop-highlight", highlight);
    g_signal_connect(motion, "enter", G_CALLBACK(show_split_drop_highlight), NULL);
    g_signal_connect(motion, "motion", G_CALLBACK(show_split_drop_highlight), NULL);
    g_signal_connect(motion, "leave", G_CALLBACK(hide_split_drop_highlight), NULL);
    gtk_widget_add_controller(target, motion);

    return target;
}

static void add_split_drop_zone(
    GtkOverlay *overlay,
    GtkWidget *target,
    GtkAlign halign,
    GtkAlign valign,
    int width,
    int height
) {
    gtk_widget_set_halign(target, halign);
    gtk_widget_set_valign(target, valign);
    gtk_widget_set_size_request(target, width, height);
    gtk_overlay_add_overlay(overlay, target);
}

static GtkWidget *create_split_overlay(KosmosMainWindow *self, AdwTabView *tab_view, guint64 pane_id) {
    GtkWidget *overlay = gtk_overlay_new();
    gtk_widget_set_hexpand(overlay, TRUE);
    gtk_widget_set_vexpand(overlay, TRUE);
    gtk_overlay_set_child(GTK_OVERLAY(overlay), GTK_WIDGET(tab_view));

    add_split_drop_zone(
        GTK_OVERLAY(overlay),
        create_split_drop_target(self, pane_id, KOSMOS_IPC_SPLIT_AXIS_HORIZONTAL, TRUE),
        GTK_ALIGN_START,
        GTK_ALIGN_FILL,
        96,
        -1
    );
    add_split_drop_zone(
        GTK_OVERLAY(overlay),
        create_split_drop_target(self, pane_id, KOSMOS_IPC_SPLIT_AXIS_HORIZONTAL, FALSE),
        GTK_ALIGN_END,
        GTK_ALIGN_FILL,
        96,
        -1
    );
    add_split_drop_zone(
        GTK_OVERLAY(overlay),
        create_split_drop_target(self, pane_id, KOSMOS_IPC_SPLIT_AXIS_VERTICAL, TRUE),
        GTK_ALIGN_FILL,
        GTK_ALIGN_START,
        -1,
        96
    );
    add_split_drop_zone(
        GTK_OVERLAY(overlay),
        create_split_drop_target(self, pane_id, KOSMOS_IPC_SPLIT_AXIS_VERTICAL, FALSE),
        GTK_ALIGN_FILL,
        GTK_ALIGN_END,
        -1,
        96
    );

    return overlay;
}

static GtkWidget *create_tabbed_pane(KosmosMainWindow *self, JsonObject *pane, guint64 workspace_id, gboolean is_active_pane) {
    guint64 pane_id = 0;
    guint64 active_tab_id = 0;
    JsonArray *tabs = get_array_member(pane, "tabs");

    if (!get_uint_member(pane, "id", &pane_id) || !get_uint_member(pane, "activeTabId", &active_tab_id) || tabs == NULL) {
        return create_label("Invalid pane snapshot.", "error");
    }

    GtkWidget *container = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    gtk_widget_set_hexpand(container, TRUE);
    gtk_widget_set_vexpand(container, TRUE);
    gtk_widget_set_size_request(container, KOSMOS_MIN_PANE_WIDTH, KOSMOS_MIN_PANE_HEIGHT);

    (void)is_active_pane;

    AdwTabView *tab_view = adw_tab_view_new();
    AdwTabBar *tab_bar = adw_tab_bar_new();
    GtkWidget *new_tab_button = create_new_tab_button(workspace_id, pane_id);
    adw_tab_bar_set_view(tab_bar, tab_view);
    adw_tab_bar_set_end_action_widget(tab_bar, new_tab_button);
    adw_tab_bar_set_autohide(tab_bar, FALSE);
    adw_tab_bar_set_expand_tabs(tab_bar, FALSE);
    adw_tab_view_set_shortcuts(
        tab_view,
        ADW_TAB_VIEW_SHORTCUT_CONTROL_TAB |
            ADW_TAB_VIEW_SHORTCUT_CONTROL_SHIFT_TAB |
            ADW_TAB_VIEW_SHORTCUT_CONTROL_PAGE_UP |
            ADW_TAB_VIEW_SHORTCUT_CONTROL_PAGE_DOWN |
            ADW_TAB_VIEW_SHORTCUT_ALT_DIGITS |
            ADW_TAB_VIEW_SHORTCUT_ALT_ZERO
    );
    gtk_widget_set_hexpand(GTK_WIDGET(tab_view), TRUE);
    gtk_widget_set_vexpand(GTK_WIDGET(tab_view), TRUE);

    guint tab_count = json_array_get_length(tabs);
    AdwTabPage *active_page = NULL;
    for (guint index = 0; index < tab_count; index++) {
        JsonNode *tab_node = json_array_get_element(tabs, index);
        if (tab_node == NULL || !JSON_NODE_HOLDS_OBJECT(tab_node)) {
            continue;
        }

        JsonObject *tab = json_node_get_object(tab_node);
        guint64 tab_id = 0;
        const char *title = NULL;
        const char *kind = NULL;
        if (!get_uint_member(tab, "id", &tab_id) || !get_string_member(tab, "title", &title)) {
            continue;
        }
        get_string_member(tab, "kind", &kind);

        GtkWidget *content = create_tab_content(kind);
        AdwTabPage *page = adw_tab_view_append(tab_view, content);
        configure_tab_page(page, workspace_id, pane_id, tab_id, title);

        if (tab_id == active_tab_id) {
            active_page = page;
        }
    }

    if (active_page != NULL) {
        adw_tab_view_set_selected_page(tab_view, active_page);
    }

    register_pane_view(self, pane_id, tab_view);
    set_uint64_data(G_OBJECT(container), "pane-id", pane_id);
    g_object_set_data(G_OBJECT(container), "tab-view", tab_view);
    g_object_set_data(G_OBJECT(tab_view), "pane-container", container);
    g_object_set_data(G_OBJECT(tab_view), "tab-bar", GTK_WIDGET(tab_bar));
    g_object_set_data(G_OBJECT(tab_view), "new-tab-button", new_tab_button);

    add_tab_bar_activation_controller(self, GTK_WIDGET(tab_bar), tab_view);
    configure_single_tab_pane_drag(
        self,
        GTK_WIDGET(tab_bar),
        new_tab_button,
        tab_view,
        workspace_id,
        pane_id,
        (int)tab_count,
        active_page
    );

    gtk_box_append(GTK_BOX(container), GTK_WIDGET(tab_bar));
    gtk_box_append(GTK_BOX(container), create_split_overlay(self, tab_view, pane_id));
    g_signal_connect(tab_view, "notify::selected-page", G_CALLBACK(activate_selected_tab), self);
    g_signal_connect(tab_view, "notify::is-transferring-page", G_CALLBACK(tab_transfer_changed), NULL);
    g_signal_connect(tab_view, "close-page", G_CALLBACK(close_tab), self);
    g_signal_connect(tab_view, "create-window", G_CALLBACK(create_split_sink_for_detached_tab), self);
    g_signal_connect(tab_view, "page-detached", G_CALLBACK(tab_detached), NULL);
    g_signal_connect(tab_view, "page-reordered", G_CALLBACK(reorder_tab), self);
    g_signal_connect(new_tab_button, "clicked", G_CALLBACK(open_blank_tab), self);

    return container;
}

static gboolean apply_paned_ratio(GtkWidget *widget, GdkFrameClock *frame_clock, gpointer user_data) {
    (void)frame_clock;

    PanedRatio *ratio = user_data;
    int size = ratio->orientation == GTK_ORIENTATION_HORIZONTAL
        ? gtk_widget_get_width(widget)
        : gtk_widget_get_height(widget);

    if (size <= 1) {
        return G_SOURCE_CONTINUE;
    }

    gtk_paned_set_position(GTK_PANED(widget), (int)(size * ratio->ratio));
    return G_SOURCE_REMOVE;
}

static void set_paned_ratio_after_layout(GtkWidget *paned, GtkOrientation orientation, double ratio) {
    PanedRatio *ratio_data = g_new(PanedRatio, 1);
    ratio_data->orientation = orientation;
    ratio_data->ratio = CLAMP(ratio, 0.05, 0.95);
    gtk_widget_add_tick_callback(paned, apply_paned_ratio, ratio_data, g_free);
}

static GtkWidget *create_pane_node(
    KosmosMainWindow *self,
    JsonObject *node,
    guint64 workspace_id,
    guint64 active_pane_id,
    const char *path,
    GHashTable *reusable_panes
) {
    const char *type = NULL;
    if (!get_string_member(node, "type", &type)) {
        return create_label("Invalid pane node.", "error");
    }

    if (g_strcmp0(type, "leaf") == 0) {
        JsonObject *pane = get_object_member(node, "pane");
        guint64 pane_id = 0;
        if (pane == NULL || !get_uint_member(pane, "id", &pane_id)) {
            return create_label("Invalid pane leaf.", "error");
        }

        GtkWidget *reusable_pane = reusable_panes == NULL ? NULL : g_hash_table_lookup(reusable_panes, &pane_id);
        if (reusable_pane != NULL && gtk_widget_get_parent(reusable_pane) == NULL) {
            AdwTabView *tab_view = g_object_get_data(G_OBJECT(reusable_pane), "tab-view");
            if (tab_view != NULL && update_tab_view_from_pane_view(self, tab_view, pane, workspace_id, active_pane_id, FALSE)) {
                return reusable_pane;
            }
        }

        return create_tabbed_pane(self, pane, workspace_id, pane_id == active_pane_id);
    }

    if (g_strcmp0(type, "split") == 0) {
        const char *axis = NULL;
        double ratio = 0.5;
        JsonObject *first = get_object_member(node, "first");
        JsonObject *second = get_object_member(node, "second");
        if (!get_string_member(node, "axis", &axis) || first == NULL || second == NULL) {
            return create_label("Invalid pane split.", "error");
        }

        get_double_member(node, "ratio", &ratio);
        get_paned_ratio(self, path, &ratio);

        GtkOrientation orientation = g_strcmp0(axis, "vertical") == 0
            ? GTK_ORIENTATION_VERTICAL
            : GTK_ORIENTATION_HORIZONTAL;
        char *first_path = g_strconcat(path, "/first", NULL);
        char *second_path = g_strconcat(path, "/second", NULL);
        GtkWidget *paned = gtk_paned_new(orientation);
        gtk_widget_set_hexpand(paned, TRUE);
        gtk_widget_set_vexpand(paned, TRUE);
        gtk_paned_set_shrink_start_child(GTK_PANED(paned), FALSE);
        gtk_paned_set_shrink_end_child(GTK_PANED(paned), FALSE);
        g_object_set_data_full(G_OBJECT(paned), "pane-path", g_strdup(path), g_free);
        gtk_paned_set_start_child(GTK_PANED(paned), create_pane_node(self, first, workspace_id, active_pane_id, first_path, reusable_panes));
        gtk_paned_set_end_child(GTK_PANED(paned), create_pane_node(self, second, workspace_id, active_pane_id, second_path, reusable_panes));
        set_paned_ratio_after_layout(paned, orientation, ratio);
        g_free(first_path);
        g_free(second_path);
        return paned;
    }

    return create_label("Unsupported pane node.", "error");
}

static gboolean widget_is_pane_leaf(GtkWidget *widget, guint64 pane_id) {
    guint64 widget_pane_id = 0;

    return get_uint64_data(G_OBJECT(widget), "pane-id", &widget_pane_id) && widget_pane_id == pane_id;
}

static GtkWidget *reconcile_pane_node(
    KosmosMainWindow *self,
    GtkWidget *current,
    JsonObject *node,
    guint64 workspace_id,
    guint64 active_pane_id,
    const char *path
) {
    const char *type = NULL;
    if (!get_string_member(node, "type", &type)) {
        return create_label("Invalid pane node.", "error");
    }

    if (g_strcmp0(type, "leaf") == 0) {
        JsonObject *pane = get_object_member(node, "pane");
        guint64 pane_id = 0;
        if (pane == NULL || !get_uint_member(pane, "id", &pane_id)) {
            return create_label("Invalid pane leaf.", "error");
        }

        if (current != NULL && widget_is_pane_leaf(current, pane_id)) {
            AdwTabView *tab_view = g_object_get_data(G_OBJECT(current), "tab-view");
            if (tab_view != NULL && update_tab_view_from_pane_view(self, tab_view, pane, workspace_id, active_pane_id, FALSE)) {
                return current;
            }
        }

        return create_tabbed_pane(self, pane, workspace_id, pane_id == active_pane_id);
    }

    if (g_strcmp0(type, "split") == 0) {
        const char *axis = NULL;
        double ratio = 0.5;
        JsonObject *first = get_object_member(node, "first");
        JsonObject *second = get_object_member(node, "second");
        if (!get_string_member(node, "axis", &axis) || first == NULL || second == NULL) {
            return create_label("Invalid pane split.", "error");
        }

        get_double_member(node, "ratio", &ratio);
        get_paned_ratio(self, path, &ratio);

        GtkOrientation orientation = g_strcmp0(axis, "vertical") == 0
            ? GTK_ORIENTATION_VERTICAL
            : GTK_ORIENTATION_HORIZONTAL;
        char *first_path = g_strconcat(path, "/first", NULL);
        char *second_path = g_strconcat(path, "/second", NULL);

        if (!GTK_IS_PANED(current)) {
            GtkWidget *created = create_pane_node(self, node, workspace_id, active_pane_id, path, NULL);
            g_free(first_path);
            g_free(second_path);
            return created;
        }

        gtk_orientable_set_orientation(GTK_ORIENTABLE(current), orientation);
        gtk_paned_set_shrink_start_child(GTK_PANED(current), FALSE);
        gtk_paned_set_shrink_end_child(GTK_PANED(current), FALSE);
        g_object_set_data_full(G_OBJECT(current), "pane-path", g_strdup(path), g_free);

        GtkWidget *old_start = gtk_paned_get_start_child(GTK_PANED(current));
        GtkWidget *new_start = reconcile_pane_node(self, old_start, first, workspace_id, active_pane_id, first_path);
        if (new_start != old_start) {
            gtk_paned_set_start_child(GTK_PANED(current), new_start);
        }

        GtkWidget *old_end = gtk_paned_get_end_child(GTK_PANED(current));
        GtkWidget *new_end = reconcile_pane_node(self, old_end, second, workspace_id, active_pane_id, second_path);
        if (new_end != old_end) {
            gtk_paned_set_end_child(GTK_PANED(current), new_end);
        }

        set_paned_ratio_after_layout(current, orientation, ratio);
        g_free(first_path);
        g_free(second_path);
        return current;
    }

    return create_label("Unsupported pane node.", "error");
}

static void render_active_workspace(KosmosMainWindow *self, JsonObject *workspace) {
    remember_paned_ratios(self);

    if (workspace == NULL) {
        g_hash_table_remove_all(self->paned_ratios);
        clear_content_area(self);
        return;
    }

    guint64 workspace_id = 0;
    guint64 active_pane_id = 0;
    JsonObject *root = get_object_member(workspace, "root");

    if (!get_uint_member(workspace, "id", &workspace_id) ||
        !get_uint_member(workspace, "activePaneId", &active_pane_id) ||
        root == NULL) {
        set_status(self, "Invalid workspace snapshot.");
        return;
    }

    prune_paned_ratios_for_root(self, root);

    GtkWidget *current_root = gtk_widget_get_first_child(self->content_area);
    if (current_root != NULL) {
        g_hash_table_remove_all(self->pane_views);
        GtkWidget *pane_tree = reconcile_pane_node(self, current_root, root, workspace_id, active_pane_id, "root");
        if (pane_tree != current_root) {
            gtk_box_remove(GTK_BOX(self->content_area), current_root);
            gtk_box_append(GTK_BOX(self->content_area), pane_tree);
        }
        return;
    }

    GtkWidget *pane_tree = create_pane_node(self, root, workspace_id, active_pane_id, "root", NULL);
    gtk_box_append(GTK_BOX(self->content_area), pane_tree);
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
    JsonObject *active_workspace = find_active_workspace(workspaces, active_workspace_id, has_active_workspace);
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

    (void)workspace_count;
    if (active_workspace != NULL) {
        char *layout_signature = create_layout_signature(active_workspace);
        if (layout_signature != NULL &&
            self->layout_signature != NULL &&
            g_strcmp0(layout_signature, self->layout_signature) == 0 &&
            update_active_workspace_in_place(self, active_workspace)) {
            g_free(layout_signature);
            return;
        }

        render_active_workspace(self, active_workspace);
        g_clear_pointer(&self->layout_signature, g_free);
        self->layout_signature = layout_signature;
        return;
    }

    render_active_workspace(self, active_workspace);
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
    self->paned_ratios = g_hash_table_new_full(g_str_hash, g_str_equal, g_free, g_free);
    self->pane_views = g_hash_table_new_full(g_int64_hash, g_int64_equal, g_free, NULL);
    install_split_drop_css(GTK_WIDGET(self));

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

    clear_detached_tab_transfer(self);
    g_clear_object(&self->ipc_client);
    g_clear_pointer(&self->workspace_buttons, g_hash_table_unref);
    g_clear_pointer(&self->paned_ratios, g_hash_table_unref);
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
