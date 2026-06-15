#include "tabs/kosmos_file_tree_tab.h"

#define KOSMOS_TYPE_FILE_TREE_ITEM (kosmos_file_tree_item_get_type())
#define KOSMOS_FILE_TREE_ITEM(object) (G_TYPE_CHECK_INSTANCE_CAST((object), KOSMOS_TYPE_FILE_TREE_ITEM, KosmosFileTreeItem))

typedef struct _KosmosFileTreeItem KosmosFileTreeItem;
typedef struct _KosmosFileTreeItemClass KosmosFileTreeItemClass;

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

    gtk_list_item_set_selectable(list_item, FALSE);
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

    gtk_box_append(GTK_BOX(row), icon);
    gtk_box_append(GTK_BOX(row), label);
    gtk_box_append(GTK_BOX(hit_row), row);
    g_object_set_data(G_OBJECT(hit_row), "entry-row", row);
    g_object_set_data(G_OBJECT(hit_row), "entry-icon", icon);
    g_object_set_data(G_OBJECT(hit_row), "entry-label", label);

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
    (void)user_data;

    GtkTreeListRow *tree_row = gtk_list_item_get_item(list_item);
    KosmosFileTreeItem *item = KOSMOS_FILE_TREE_ITEM(gtk_tree_list_row_get_item(tree_row));
    GtkWidget *hit_row = gtk_list_item_get_child(list_item);
    GtkWidget *row = g_object_get_data(G_OBJECT(hit_row), "entry-row");
    GtkWidget *icon = g_object_get_data(G_OBJECT(hit_row), "entry-icon");
    GtkWidget *label = g_object_get_data(G_OBJECT(hit_row), "entry-label");
    guint depth = gtk_tree_list_row_get_depth(tree_row);

    gtk_widget_set_margin_start(row, 10 + (int)(depth * 18));
    gtk_image_set_from_icon_name(GTK_IMAGE(icon), entry_icon_name(item));
    gtk_label_set_text(GTK_LABEL(label), item->name);
    gtk_widget_set_tooltip_text(hit_row, item->path[0] == '\0' ? NULL : item->path);
    set_tree_row_data(hit_row, tree_row);

    if (kosmos_file_tree_item_is_message(item)) {
        gtk_widget_add_css_class(label, "dim-label");
        gtk_widget_add_css_class(icon, "dim-label");
    } else {
        gtk_widget_remove_css_class(label, "dim-label");
        gtk_widget_remove_css_class(icon, "dim-label");
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

static void file_tree_clicked(GtkGestureClick *gesture, int n_press, double x, double y, gpointer user_data) {
    (void)n_press;

    GtkWidget *root = gtk_event_controller_get_widget(GTK_EVENT_CONTROLLER(gesture));
    GtkWidget *list = GTK_WIDGET(user_data);
    graphene_point_t root_point = GRAPHENE_POINT_INIT((float)x, (float)y);
    graphene_point_t list_point = GRAPHENE_POINT_INIT(0.0f, 0.0f);
    GtkTreeListRow *tree_row = NULL;

    if (gtk_widget_compute_point(root, list, &root_point, &list_point)) {
        GtkWidget *picked = gtk_widget_pick(list, list_point.x, list_point.y, GTK_PICK_DEFAULT);
        tree_row = tree_row_for_picked_widget(picked);
    }

    if (tree_row != NULL && gtk_tree_list_row_is_expandable(tree_row)) {
        gtk_tree_list_row_set_expanded(tree_row, !gtk_tree_list_row_get_expanded(tree_row));
    }

    g_clear_object(&tree_row);
}

static void unbind_list_item(GtkSignalListItemFactory *factory, GtkListItem *list_item, gpointer user_data) {
    (void)factory;
    (void)user_data;

    GtkWidget *hit_row = gtk_list_item_get_child(list_item);
    if (hit_row != NULL) {
        g_object_set_data(G_OBJECT(hit_row), "tree-row", NULL);
    }
}

static GtkWidget *create_file_tree_list(JsonObject *root) {
    GListStore *root_store = g_list_store_new(KOSMOS_TYPE_FILE_TREE_ITEM);
    KosmosFileTreeItem *root_item = file_tree_item_from_entry(root);
    g_list_store_append(root_store, root_item);
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

    GtkListItemFactory *factory = gtk_signal_list_item_factory_new();
    g_signal_connect(factory, "setup", G_CALLBACK(setup_list_item), NULL);
    g_signal_connect(factory, "bind", G_CALLBACK(bind_list_item), NULL);
    g_signal_connect(factory, "unbind", G_CALLBACK(unbind_list_item), NULL);

    GtkSelectionModel *selection = GTK_SELECTION_MODEL(gtk_no_selection_new(G_LIST_MODEL(tree_model)));
    GtkWidget *list = gtk_list_view_new(selection, factory);

    gtk_widget_set_margin_bottom(list, 8);
    gtk_widget_set_vexpand(list, TRUE);
    gtk_widget_set_hexpand(list, TRUE);

    return list;
}

static GtkWidget *create_tree_view(JsonNode *tree) {
    if (tree == NULL || !JSON_NODE_HOLDS_OBJECT(tree)) {
        return create_status_label("File tree is unavailable.", "dim-label");
    }

    JsonObject *result = json_node_get_object(tree);
    JsonObject *root = get_object_member(result, "root");
    if (root == NULL) {
        return create_status_label("File tree response is invalid.", "error");
    }

    GtkWidget *root_widget = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    GtkWidget *scrolled = gtk_scrolled_window_new();
    GtkWidget *list = create_file_tree_list(root);
    GtkGesture *click = gtk_gesture_click_new();

    gtk_widget_set_hexpand(root_widget, TRUE);
    gtk_widget_set_vexpand(root_widget, TRUE);

    gtk_gesture_single_set_button(GTK_GESTURE_SINGLE(click), GDK_BUTTON_PRIMARY);
    gtk_event_controller_set_propagation_phase(GTK_EVENT_CONTROLLER(click), GTK_PHASE_CAPTURE);
    g_signal_connect(click, "pressed", G_CALLBACK(file_tree_clicked), list);
    gtk_widget_add_controller(root_widget, GTK_EVENT_CONTROLLER(click));

    gtk_scrolled_window_set_policy(GTK_SCROLLED_WINDOW(scrolled), GTK_POLICY_AUTOMATIC, GTK_POLICY_AUTOMATIC);
    gtk_scrolled_window_set_child(GTK_SCROLLED_WINDOW(scrolled), list);
    gtk_widget_set_hexpand(scrolled, TRUE);
    gtk_widget_set_vexpand(scrolled, TRUE);
    gtk_box_append(GTK_BOX(root_widget), scrolled);

    return root_widget;
}

GtkWidget *kosmos_file_tree_tab_create(KosmosIpcClient *ipc_client, guint64 workspace_id) {
    g_return_val_if_fail(KOSMOS_IS_IPC_CLIENT(ipc_client), NULL);

    GError *error = NULL;
    JsonNode *tree = NULL;
    if (!kosmos_ipc_client_list_file_tree(ipc_client, workspace_id, &tree, NULL, &error)) {
        const char *detail = error == NULL ? "unknown IPC error" : error->message;
        char *message = g_strdup_printf("Failed to load file tree: %s", detail);
        GtkWidget *status = create_status_label(message, "error");

        g_free(message);
        g_clear_error(&error);
        return status;
    }

    GtkWidget *view = create_tree_view(tree);
    if (tree != NULL) {
        json_node_unref(tree);
    }

    return view;
}
