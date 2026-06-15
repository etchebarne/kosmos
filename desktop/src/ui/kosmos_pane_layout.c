#include "ui/kosmos_main_window_private.h"

#define KOSMOS_PANED_RATIO_EPSILON 0.001

typedef struct {
    KosmosMainWindow *window;
    GtkOrientation orientation;
    double ratio;
    guint serial;
    guint layout_apply_serial;
    gboolean pending_layout_apply;
} PanedRatio;

static void set_paned_ratio_after_layout(KosmosMainWindow *self, GtkWidget *paned, GtkOrientation orientation, double ratio);

static GtkWidget *layout_content_area(KosmosMainWindow *self) {
    return self->staged_content_area == NULL ? self->content_area : self->staged_content_area;
}

static void reveal_hidden_layout_if_ready(KosmosMainWindow *self) {
    if (!self->hiding_layout_apply || self->pending_layout_applies > 0) {
        return;
    }

    self->hiding_layout_apply = FALSE;
    if (self->staged_content_area == NULL) {
        return;
    }

    GtkWidget *staged_content_area = g_object_ref(self->staged_content_area);
    gtk_widget_set_opacity(staged_content_area, 1.0);
    gtk_widget_set_can_target(staged_content_area, TRUE);
    gtk_overlay_remove_overlay(GTK_OVERLAY(self->content_overlay), staged_content_area);
    gtk_overlay_set_child(GTK_OVERLAY(self->content_overlay), staged_content_area);
    self->content_area = staged_content_area;
    self->staged_content_area = NULL;
    g_object_unref(staged_content_area);
}

void kosmos_pane_layout_begin_hidden_apply(KosmosMainWindow *self) {
    if (self->staged_content_area != NULL) {
        gtk_overlay_remove_overlay(GTK_OVERLAY(self->content_overlay), self->staged_content_area);
        self->staged_content_area = NULL;
    }

    self->layout_apply_serial++;
    self->pending_layout_applies = 0;
    self->hiding_layout_apply = TRUE;

    self->staged_content_area = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    gtk_widget_set_hexpand(self->staged_content_area, TRUE);
    gtk_widget_set_vexpand(self->staged_content_area, TRUE);
    gtk_widget_set_opacity(self->staged_content_area, 0.0);
    gtk_widget_set_can_target(self->staged_content_area, FALSE);
    gtk_overlay_add_overlay(GTK_OVERLAY(self->content_overlay), self->staged_content_area);
    g_hash_table_remove_all(self->pane_views);
}

void kosmos_pane_layout_finish_hidden_apply(KosmosMainWindow *self) {
    reveal_hidden_layout_if_ready(self);
}

static void finish_paned_ratio_apply(PanedRatio *ratio) {
    KosmosMainWindow *self = ratio->window;

    if (!ratio->pending_layout_apply || ratio->layout_apply_serial != self->layout_apply_serial) {
        return;
    }

    ratio->pending_layout_apply = FALSE;
    if (self->pending_layout_applies > 0) {
        self->pending_layout_applies--;
    }
    reveal_hidden_layout_if_ready(self);
}

static void paned_ratio_free(PanedRatio *ratio) {
    finish_paned_ratio_apply(ratio);
    g_object_unref(ratio->window);
    g_free(ratio);
}

static gboolean clear_applying_paned_ratio(gpointer user_data) {
    GObject *object = user_data;
    g_object_set_data(object, "applying-server-ratio", NULL);
    g_object_unref(object);
    return G_SOURCE_REMOVE;
}

static gboolean get_double_data(GObject *object, const char *key, double *value) {
    double *stored_value = g_object_get_data(object, key);

    if (stored_value == NULL) {
        return FALSE;
    }

    *value = *stored_value;
    return TRUE;
}

static void set_double_data(GObject *object, const char *key, double value) {
    double *stored_value = g_new(double, 1);
    *stored_value = value;
    g_object_set_data_full(object, key, stored_value, g_free);
}

static gboolean ratios_match(double first, double second) {
    return ABS(first - second) <= KOSMOS_PANED_RATIO_EPSILON;
}

static void remember_server_paned_ratio(GtkWidget *paned, guint64 workspace_id, guint64 split_id, double ratio) {
    GObject *object = G_OBJECT(paned);
    kosmos_main_window_set_uint64_data(object, "server-ratio-workspace-id", workspace_id);
    kosmos_main_window_set_uint64_data(object, "server-ratio-split-id", split_id);
    set_double_data(object, "server-ratio", ratio);
}

static gboolean paned_already_has_server_ratio(GtkWidget *paned, guint64 workspace_id, guint64 split_id, double ratio) {
    guint64 stored_workspace_id = 0;
    guint64 stored_split_id = 0;
    double stored_ratio = 0.0;

    return kosmos_main_window_get_uint64_data(G_OBJECT(paned), "server-ratio-workspace-id", &stored_workspace_id) &&
        kosmos_main_window_get_uint64_data(G_OBJECT(paned), "server-ratio-split-id", &stored_split_id) &&
        get_double_data(G_OBJECT(paned), "server-ratio", &stored_ratio) &&
        stored_workspace_id == workspace_id &&
        stored_split_id == split_id &&
        ratios_match(stored_ratio, ratio);
}

static guint next_paned_ratio_serial(GtkWidget *paned) {
    guint serial = GPOINTER_TO_UINT(g_object_get_data(G_OBJECT(paned), "server-ratio-serial")) + 1;
    g_object_set_data(G_OBJECT(paned), "server-ratio-serial", GUINT_TO_POINTER(serial));
    return serial;
}

static void request_split_resize(KosmosMainWindow *self, guint64 workspace_id, guint64 split_id, double ratio) {
    if (!kosmos_main_window_ensure_connected(self)) {
        return;
    }

    GError *error = NULL;
    kosmos_ipc_client_resize_pane_split(
        self->ipc_client,
        workspace_id,
        split_id,
        ratio,
        NULL,
        NULL,
        &error
    );

    if (error != NULL) {
        g_warning("Failed to resize pane split: %s", error->message);
        g_clear_error(&error);
    }
}

static void paned_position_changed(GObject *object, GParamSpec *pspec, gpointer user_data) {
    (void)pspec;

    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    if (self->applying_server_state || g_object_get_data(object, "applying-server-ratio") != NULL) {
        return;
    }

    GtkWidget *widget = GTK_WIDGET(object);
    GtkOrientation orientation = gtk_orientable_get_orientation(GTK_ORIENTABLE(widget));
    int size = orientation == GTK_ORIENTATION_HORIZONTAL
        ? gtk_widget_get_width(widget)
        : gtk_widget_get_height(widget);
    if (size <= 1) {
        return;
    }

    guint64 workspace_id = 0;
    guint64 split_id = 0;
    if (!kosmos_main_window_get_uint64_data(object, "workspace-id", &workspace_id) ||
        !kosmos_main_window_get_uint64_data(object, "split-id", &split_id)) {
        return;
    }

    double ratio = CLAMP((double)gtk_paned_get_position(GTK_PANED(widget)) / size, 0.05, 0.95);
    if (paned_already_has_server_ratio(widget, workspace_id, split_id, ratio)) {
        return;
    }

    remember_server_paned_ratio(widget, workspace_id, split_id, ratio);
    request_split_resize(self, workspace_id, split_id, ratio);
}

static void configure_paned_split(KosmosMainWindow *self, GtkWidget *paned, guint64 workspace_id, guint64 split_id) {
    kosmos_main_window_set_uint64_data(G_OBJECT(paned), "workspace-id", workspace_id);
    kosmos_main_window_set_uint64_data(G_OBJECT(paned), "split-id", split_id);

    if (g_object_get_data(G_OBJECT(paned), "split-resize-handler-installed") != NULL) {
        return;
    }

    g_signal_connect(paned, "notify::position", G_CALLBACK(paned_position_changed), self);
    g_object_set_data(G_OBJECT(paned), "split-resize-handler-installed", GINT_TO_POINTER(1));
}

static gboolean append_layout_signature(GString *signature, JsonObject *node) {
    const char *type = NULL;
    if (!kosmos_json_get_string_member(node, "type", &type)) {
        return FALSE;
    }

    if (g_strcmp0(type, "leaf") == 0) {
        JsonObject *pane = kosmos_json_get_object_member(node, "pane");
        guint64 pane_id = 0;
        if (pane == NULL || !kosmos_json_get_uint_member(pane, "id", &pane_id)) {
            return FALSE;
        }

        g_string_append_printf(signature, "L:%" G_GUINT64_FORMAT, pane_id);
        return TRUE;
    }

    if (g_strcmp0(type, "split") == 0) {
        const char *axis = NULL;
        guint64 split_id = 0;
        JsonObject *first = kosmos_json_get_object_member(node, "first");
        JsonObject *second = kosmos_json_get_object_member(node, "second");
        if (!kosmos_json_get_uint_member(node, "id", &split_id) ||
            !kosmos_json_get_string_member(node, "axis", &axis) ||
            first == NULL ||
            second == NULL) {
            return FALSE;
        }

        g_string_append_printf(signature, "S:%" G_GUINT64_FORMAT ":%s(", split_id, axis);
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

char *kosmos_pane_layout_create_signature(JsonObject *workspace) {
    JsonObject *root = kosmos_json_get_object_member(workspace, "root");
    guint64 workspace_id = 0;
    if (root == NULL || !kosmos_json_get_uint_member(workspace, "id", &workspace_id)) {
        return NULL;
    }

    GString *signature = g_string_new(NULL);
    g_string_append_printf(signature, "W:%" G_GUINT64_FORMAT ";", workspace_id);
    if (!append_layout_signature(signature, root)) {
        g_string_free(signature, TRUE);
        return NULL;
    }

    return g_string_free(signature, FALSE);
}

static gboolean update_pane_node_in_place(
    KosmosMainWindow *self,
    GtkWidget *current,
    JsonObject *node,
    guint64 workspace_id,
    guint64 active_pane_id
) {
    const char *type = NULL;
    if (current == NULL || !kosmos_json_get_string_member(node, "type", &type)) {
        return FALSE;
    }

    if (g_strcmp0(type, "leaf") == 0) {
        JsonObject *pane = kosmos_json_get_object_member(node, "pane");
        return pane != NULL && kosmos_tabbed_pane_update_from_pane(self, pane, workspace_id, active_pane_id);
    }

    if (g_strcmp0(type, "split") == 0) {
        const char *axis = NULL;
        guint64 split_id = 0;
        double ratio = 0.5;
        JsonObject *first = kosmos_json_get_object_member(node, "first");
        JsonObject *second = kosmos_json_get_object_member(node, "second");
        if (!GTK_IS_PANED(current) ||
            !kosmos_json_get_uint_member(node, "id", &split_id) ||
            !kosmos_json_get_string_member(node, "axis", &axis) ||
            first == NULL ||
            second == NULL) {
            return FALSE;
        }

        kosmos_json_get_double_member(node, "ratio", &ratio);
        GtkOrientation orientation = g_strcmp0(axis, "vertical") == 0
            ? GTK_ORIENTATION_VERTICAL
            : GTK_ORIENTATION_HORIZONTAL;
        gtk_orientable_set_orientation(GTK_ORIENTABLE(current), orientation);
        gtk_paned_set_shrink_start_child(GTK_PANED(current), FALSE);
        gtk_paned_set_shrink_end_child(GTK_PANED(current), FALSE);
        configure_paned_split(self, current, workspace_id, split_id);
        set_paned_ratio_after_layout(self, current, orientation, ratio);

        return update_pane_node_in_place(self, gtk_paned_get_start_child(GTK_PANED(current)), first, workspace_id, active_pane_id) &&
            update_pane_node_in_place(self, gtk_paned_get_end_child(GTK_PANED(current)), second, workspace_id, active_pane_id);
    }

    return FALSE;
}

gboolean kosmos_pane_layout_update_active_workspace_in_place(KosmosMainWindow *self, JsonObject *workspace) {
    guint64 workspace_id = 0;
    guint64 active_pane_id = 0;
    JsonObject *root = kosmos_json_get_object_member(workspace, "root");

    if (!kosmos_json_get_uint_member(workspace, "id", &workspace_id) ||
        !kosmos_json_get_uint_member(workspace, "activePaneId", &active_pane_id) ||
        root == NULL) {
        return FALSE;
    }

    GtkWidget *current_root = gtk_widget_get_first_child(layout_content_area(self));
    return update_pane_node_in_place(self, current_root, root, workspace_id, active_pane_id);
}

static gboolean apply_paned_ratio(GtkWidget *widget, GdkFrameClock *frame_clock, gpointer user_data) {
    (void)frame_clock;

    PanedRatio *ratio = user_data;
    guint current_serial = GPOINTER_TO_UINT(g_object_get_data(G_OBJECT(widget), "server-ratio-serial"));
    if (current_serial != ratio->serial) {
        finish_paned_ratio_apply(ratio);
        return G_SOURCE_REMOVE;
    }

    int size = ratio->orientation == GTK_ORIENTATION_HORIZONTAL
        ? gtk_widget_get_width(widget)
        : gtk_widget_get_height(widget);

    if (size <= 1) {
        return G_SOURCE_CONTINUE;
    }

    int position = (int)((size * ratio->ratio) + 0.5);
    if (ABS(gtk_paned_get_position(GTK_PANED(widget)) - position) <= 1) {
        g_object_set_data(G_OBJECT(widget), "applying-server-ratio", NULL);
        finish_paned_ratio_apply(ratio);
        return G_SOURCE_REMOVE;
    }

    g_object_set_data(G_OBJECT(widget), "applying-server-ratio", GINT_TO_POINTER(1));
    gtk_paned_set_position(GTK_PANED(widget), position);
    g_idle_add_full(
        G_PRIORITY_DEFAULT_IDLE,
        clear_applying_paned_ratio,
        g_object_ref(widget),
        NULL
    );
    finish_paned_ratio_apply(ratio);
    return G_SOURCE_REMOVE;
}

static void set_paned_ratio_after_layout(KosmosMainWindow *self, GtkWidget *paned, GtkOrientation orientation, double ratio) {
    guint64 workspace_id = 0;
    guint64 split_id = 0;
    ratio = CLAMP(ratio, 0.05, 0.95);

    if (kosmos_main_window_get_uint64_data(G_OBJECT(paned), "workspace-id", &workspace_id) &&
        kosmos_main_window_get_uint64_data(G_OBJECT(paned), "split-id", &split_id) &&
        paned_already_has_server_ratio(paned, workspace_id, split_id, ratio)) {
        return;
    }

    if (workspace_id != 0 && split_id != 0) {
        remember_server_paned_ratio(paned, workspace_id, split_id, ratio);
    }

    g_object_set_data(G_OBJECT(paned), "applying-server-ratio", GINT_TO_POINTER(1));

    PanedRatio *ratio_data = g_new(PanedRatio, 1);
    ratio_data->window = g_object_ref(self);
    ratio_data->orientation = orientation;
    ratio_data->ratio = ratio;
    ratio_data->serial = next_paned_ratio_serial(paned);
    ratio_data->layout_apply_serial = self->layout_apply_serial;
    ratio_data->pending_layout_apply = self->hiding_layout_apply;
    if (ratio_data->pending_layout_apply) {
        self->pending_layout_applies++;
    }

    gtk_widget_add_tick_callback(paned, apply_paned_ratio, ratio_data, (GDestroyNotify)paned_ratio_free);
}

static GtkWidget *create_pane_node(
    KosmosMainWindow *self,
    JsonObject *node,
    guint64 workspace_id,
    guint64 active_pane_id,
    const char *path,
    GHashTable *reusable_panes
);

static GtkWidget *create_split_node(
    KosmosMainWindow *self,
    JsonObject *node,
    guint64 workspace_id,
    guint64 active_pane_id,
    const char *path,
    GHashTable *reusable_panes
) {
    const char *axis = NULL;
    guint64 split_id = 0;
    double ratio = 0.5;
    JsonObject *first = kosmos_json_get_object_member(node, "first");
    JsonObject *second = kosmos_json_get_object_member(node, "second");
    if (!kosmos_json_get_uint_member(node, "id", &split_id) ||
        !kosmos_json_get_string_member(node, "axis", &axis) ||
        first == NULL ||
        second == NULL) {
        return kosmos_main_window_create_label("Invalid pane split.", "error");
    }

    kosmos_json_get_double_member(node, "ratio", &ratio);

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
    configure_paned_split(self, paned, workspace_id, split_id);
    gtk_paned_set_start_child(GTK_PANED(paned), create_pane_node(self, first, workspace_id, active_pane_id, first_path, reusable_panes));
    gtk_paned_set_end_child(GTK_PANED(paned), create_pane_node(self, second, workspace_id, active_pane_id, second_path, reusable_panes));
    set_paned_ratio_after_layout(self, paned, orientation, ratio);
    g_free(first_path);
    g_free(second_path);
    return paned;
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
    if (!kosmos_json_get_string_member(node, "type", &type)) {
        return kosmos_main_window_create_label("Invalid pane node.", "error");
    }

    if (g_strcmp0(type, "leaf") == 0) {
        JsonObject *pane = kosmos_json_get_object_member(node, "pane");
        guint64 pane_id = 0;
        if (pane == NULL || !kosmos_json_get_uint_member(pane, "id", &pane_id)) {
            return kosmos_main_window_create_label("Invalid pane leaf.", "error");
        }

        GtkWidget *reusable_pane = reusable_panes == NULL ? NULL : g_hash_table_lookup(reusable_panes, &pane_id);
        if (reusable_pane != NULL && gtk_widget_get_parent(reusable_pane) == NULL) {
            AdwTabView *tab_view = g_object_get_data(G_OBJECT(reusable_pane), "tab-view");
            if (tab_view != NULL && kosmos_tabbed_pane_update_from_pane_view(self, tab_view, pane, workspace_id, active_pane_id, FALSE)) {
                return reusable_pane;
            }
        }

        return kosmos_tabbed_pane_create(self, pane, workspace_id, pane_id == active_pane_id);
    }

    if (g_strcmp0(type, "split") == 0) {
        return create_split_node(self, node, workspace_id, active_pane_id, path, reusable_panes);
    }

    return kosmos_main_window_create_label("Unsupported pane node.", "error");
}

static gboolean widget_is_pane_leaf(GtkWidget *widget, guint64 pane_id) {
    guint64 widget_pane_id = 0;

    return kosmos_main_window_get_uint64_data(G_OBJECT(widget), "pane-id", &widget_pane_id) && widget_pane_id == pane_id;
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
    if (!kosmos_json_get_string_member(node, "type", &type)) {
        return kosmos_main_window_create_label("Invalid pane node.", "error");
    }

    if (g_strcmp0(type, "leaf") == 0) {
        JsonObject *pane = kosmos_json_get_object_member(node, "pane");
        guint64 pane_id = 0;
        if (pane == NULL || !kosmos_json_get_uint_member(pane, "id", &pane_id)) {
            return kosmos_main_window_create_label("Invalid pane leaf.", "error");
        }

        if (current != NULL && widget_is_pane_leaf(current, pane_id)) {
            AdwTabView *tab_view = g_object_get_data(G_OBJECT(current), "tab-view");
            if (tab_view != NULL && kosmos_tabbed_pane_update_from_pane_view(self, tab_view, pane, workspace_id, active_pane_id, FALSE)) {
                return current;
            }
        }

        return kosmos_tabbed_pane_create(self, pane, workspace_id, pane_id == active_pane_id);
    }

    if (g_strcmp0(type, "split") == 0) {
        const char *axis = NULL;
        guint64 split_id = 0;
        double ratio = 0.5;
        JsonObject *first = kosmos_json_get_object_member(node, "first");
        JsonObject *second = kosmos_json_get_object_member(node, "second");
        if (!kosmos_json_get_uint_member(node, "id", &split_id) ||
            !kosmos_json_get_string_member(node, "axis", &axis) ||
            first == NULL ||
            second == NULL) {
            return kosmos_main_window_create_label("Invalid pane split.", "error");
        }

        kosmos_json_get_double_member(node, "ratio", &ratio);

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
        configure_paned_split(self, current, workspace_id, split_id);

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

        set_paned_ratio_after_layout(self, current, orientation, ratio);
        g_free(first_path);
        g_free(second_path);
        return current;
    }

    return kosmos_main_window_create_label("Unsupported pane node.", "error");
}

void kosmos_pane_layout_render_active_workspace(KosmosMainWindow *self, JsonObject *workspace) {
    if (workspace == NULL) {
        kosmos_main_window_clear_content_area(self);
        return;
    }

    guint64 workspace_id = 0;
    guint64 active_pane_id = 0;
    JsonObject *root = kosmos_json_get_object_member(workspace, "root");

    if (!kosmos_json_get_uint_member(workspace, "id", &workspace_id) ||
        !kosmos_json_get_uint_member(workspace, "activePaneId", &active_pane_id) ||
        root == NULL) {
        kosmos_main_window_set_status(self, "Invalid workspace snapshot.");
        return;
    }

    GtkWidget *content_area = layout_content_area(self);
    GtkWidget *current_root = gtk_widget_get_first_child(content_area);
    if (current_root != NULL) {
        g_hash_table_remove_all(self->pane_views);
        GtkWidget *pane_tree = reconcile_pane_node(self, current_root, root, workspace_id, active_pane_id, "root");
        if (pane_tree != current_root) {
            gtk_box_remove(GTK_BOX(content_area), current_root);
            gtk_box_append(GTK_BOX(content_area), pane_tree);
        }
        return;
    }

    GtkWidget *pane_tree = create_pane_node(self, root, workspace_id, active_pane_id, "root", NULL);
    gtk_box_append(GTK_BOX(content_area), pane_tree);
}
