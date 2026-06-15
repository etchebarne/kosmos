#include "ui/kosmos_main_window_private.h"

typedef struct {
    GtkOrientation orientation;
    double ratio;
} PanedRatio;

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
        JsonObject *first = kosmos_json_get_object_member(node, "first");
        JsonObject *second = kosmos_json_get_object_member(node, "second");
        if (!kosmos_json_get_string_member(node, "axis", &axis) || first == NULL || second == NULL) {
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

char *kosmos_pane_layout_create_signature(JsonObject *workspace) {
    JsonObject *root = kosmos_json_get_object_member(workspace, "root");
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
    if (!kosmos_json_get_string_member(node, "type", &type)) {
        return;
    }

    if (g_strcmp0(type, "split") != 0) {
        return;
    }

    JsonObject *first = kosmos_json_get_object_member(node, "first");
    JsonObject *second = kosmos_json_get_object_member(node, "second");
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

static gboolean update_pane_node_in_place(KosmosMainWindow *self, JsonObject *node, guint64 workspace_id, guint64 active_pane_id) {
    const char *type = NULL;
    if (!kosmos_json_get_string_member(node, "type", &type)) {
        return FALSE;
    }

    if (g_strcmp0(type, "leaf") == 0) {
        JsonObject *pane = kosmos_json_get_object_member(node, "pane");
        return pane != NULL && kosmos_tabbed_pane_update_from_pane(self, pane, workspace_id, active_pane_id);
    }

    if (g_strcmp0(type, "split") == 0) {
        JsonObject *first = kosmos_json_get_object_member(node, "first");
        JsonObject *second = kosmos_json_get_object_member(node, "second");
        return first != NULL &&
            second != NULL &&
            update_pane_node_in_place(self, first, workspace_id, active_pane_id) &&
            update_pane_node_in_place(self, second, workspace_id, active_pane_id);
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

    return update_pane_node_in_place(self, root, workspace_id, active_pane_id);
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
    double ratio = 0.5;
    JsonObject *first = kosmos_json_get_object_member(node, "first");
    JsonObject *second = kosmos_json_get_object_member(node, "second");
    if (!kosmos_json_get_string_member(node, "axis", &axis) || first == NULL || second == NULL) {
        return kosmos_main_window_create_label("Invalid pane split.", "error");
    }

    kosmos_json_get_double_member(node, "ratio", &ratio);
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
        double ratio = 0.5;
        JsonObject *first = kosmos_json_get_object_member(node, "first");
        JsonObject *second = kosmos_json_get_object_member(node, "second");
        if (!kosmos_json_get_string_member(node, "axis", &axis) || first == NULL || second == NULL) {
            return kosmos_main_window_create_label("Invalid pane split.", "error");
        }

        kosmos_json_get_double_member(node, "ratio", &ratio);
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

    return kosmos_main_window_create_label("Unsupported pane node.", "error");
}

void kosmos_pane_layout_render_active_workspace(KosmosMainWindow *self, JsonObject *workspace) {
    remember_paned_ratios(self);

    if (workspace == NULL) {
        g_hash_table_remove_all(self->paned_ratios);
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
