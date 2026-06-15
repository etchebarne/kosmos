#include "ui/kosmos_main_window_private.h"

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

void kosmos_tabbed_pane_clear_pending_activation(AdwTabView *view) {
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
    if (!kosmos_main_window_ensure_connected(self)) {
        return;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    kosmos_ipc_client_activate_tab(self->ipc_client, workspace_id, pane_id, tab_id, &state, NULL, &error);
    kosmos_main_window_apply_server_state_or_show_error(self, state, error, "Failed to activate tab");

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
        kosmos_tabbed_pane_clear_pending_activation(view);
        return G_SOURCE_REMOVE;
    }

    pending_copy = *pending;
    kosmos_tabbed_pane_clear_pending_activation(view);
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
        kosmos_tabbed_pane_clear_pending_activation(view);
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
        kosmos_tabbed_pane_clear_pending_activation(view);
    }
}

static void tab_detached(AdwTabView *view, AdwTabPage *page, int position, gpointer user_data) {
    (void)page;
    (void)position;
    (void)user_data;

    kosmos_tabbed_pane_clear_pending_activation(view);
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

    if (page == NULL || !kosmos_main_window_ensure_connected(self)) {
        return;
    }

    guint64 workspace_id = 0;
    guint64 pane_id = 0;
    guint64 tab_id = 0;
    if (!kosmos_main_window_get_uint64_data(G_OBJECT(page), "workspace-id", &workspace_id) ||
        !kosmos_main_window_get_uint64_data(G_OBJECT(page), "pane-id", &pane_id) ||
        !kosmos_main_window_get_uint64_data(G_OBJECT(page), "tab-id", &tab_id)) {
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
    if (!kosmos_main_window_get_uint64_data(G_OBJECT(page), "workspace-id", &workspace_id) ||
        !kosmos_main_window_get_uint64_data(G_OBJECT(page), "pane-id", &pane_id) ||
        !kosmos_main_window_get_uint64_data(G_OBJECT(page), "tab-id", &tab_id) ||
        !kosmos_main_window_ensure_connected(self)) {
        adw_tab_view_close_page_finish(view, page, FALSE);
        return GDK_EVENT_STOP;
    }

    GError *error = NULL;
    JsonNode *state = NULL;
    gboolean closed = kosmos_ipc_client_close_tab(self->ipc_client, workspace_id, pane_id, tab_id, &state, NULL, &error);
    adw_tab_view_close_page_finish(view, page, closed);
    kosmos_main_window_apply_server_state_or_show_error(self, state, error, "Failed to close tab");

    g_clear_error(&error);
    if (state != NULL) {
        json_node_unref(state);
    }

    return GDK_EVENT_STOP;
}

static void reorder_tab(AdwTabView *view, AdwTabPage *page, int position, gpointer user_data) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    kosmos_tabbed_pane_clear_pending_activation(view);

    if (self->applying_server_state) {
        return;
    }

    if (adw_tab_view_get_is_transferring_page(view)) {
        return;
    }

    guint64 workspace_id = 0;
    guint64 pane_id = 0;
    guint64 tab_id = 0;
    if (!kosmos_main_window_get_uint64_data(G_OBJECT(page), "workspace-id", &workspace_id) ||
        !kosmos_main_window_get_uint64_data(G_OBJECT(page), "pane-id", &pane_id) ||
        !kosmos_main_window_get_uint64_data(G_OBJECT(page), "tab-id", &tab_id) ||
        !kosmos_main_window_ensure_connected(self)) {
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

    kosmos_main_window_apply_workspace_state(self, state);
    if (state != NULL) {
        json_node_unref(state);
    }
}

static void open_blank_tab(GtkButton *button, gpointer user_data) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);

    guint64 workspace_id = 0;
    guint64 pane_id = 0;
    if (!kosmos_main_window_get_uint64_data(G_OBJECT(button), "workspace-id", &workspace_id) ||
        !kosmos_main_window_get_uint64_data(G_OBJECT(button), "pane-id", &pane_id) ||
        !kosmos_main_window_ensure_connected(self)) {
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
    kosmos_main_window_apply_server_state_or_show_error(self, state, error, "Failed to open tab");

    g_clear_error(&error);
    if (state != NULL) {
        json_node_unref(state);
    }
}

static GtkWidget *create_new_tab_button(guint64 workspace_id, guint64 pane_id) {
    GtkWidget *button = gtk_button_new_from_icon_name("list-add-symbolic");
    gtk_widget_set_tooltip_text(button, "Open blank tab");
    kosmos_main_window_set_uint64_data(G_OBJECT(button), "workspace-id", workspace_id);
    kosmos_main_window_set_uint64_data(G_OBJECT(button), "pane-id", pane_id);

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
    kosmos_main_window_set_uint64_data(G_OBJECT(page), "workspace-id", workspace_id);
    kosmos_main_window_set_uint64_data(G_OBJECT(page), "pane-id", pane_id);
    kosmos_main_window_set_uint64_data(G_OBJECT(page), "tab-id", tab_id);
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

static AdwTabPage *tab_page_for_id(AdwTabView *view, guint64 tab_id) {
    int page_count = adw_tab_view_get_n_pages(view);

    for (int index = 0; index < page_count; index++) {
        AdwTabPage *page = adw_tab_view_get_nth_page(view, index);
        guint64 page_tab_id = 0;
        if (kosmos_main_window_get_uint64_data(G_OBJECT(page), "tab-id", &page_tab_id) && page_tab_id == tab_id) {
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
            !kosmos_main_window_get_uint64_data(G_OBJECT(page), "tab-id", &page_tab_id) ||
            !kosmos_json_get_uint_member(tab, "id", &snapshot_tab_id) ||
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

gboolean kosmos_tabbed_pane_update_from_pane_view(
    KosmosMainWindow *self,
    AdwTabView *view,
    JsonObject *pane,
    guint64 workspace_id,
    guint64 active_pane_id,
    gboolean allow_append
) {
    guint64 pane_id = 0;
    guint64 active_tab_id = 0;
    JsonArray *tabs = kosmos_json_get_array_member(pane, "tabs");

    if (!kosmos_json_get_uint_member(pane, "id", &pane_id) ||
        !kosmos_json_get_uint_member(pane, "activeTabId", &active_tab_id) ||
        tabs == NULL) {
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
        if (tab == NULL ||
            !kosmos_json_get_uint_member(tab, "id", &tab_id) ||
            !kosmos_json_get_string_member(tab, "title", &title)) {
            continue;
        }
        kosmos_json_get_string_member(tab, "kind", &kind);

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
        kosmos_pane_dnd_configure_single_tab(
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
    kosmos_main_window_register_pane_view(self, pane_id, view);
    return TRUE;
}

gboolean kosmos_tabbed_pane_update_from_pane(KosmosMainWindow *self, JsonObject *pane, guint64 workspace_id, guint64 active_pane_id) {
    guint64 pane_id = 0;
    if (!kosmos_json_get_uint_member(pane, "id", &pane_id)) {
        return FALSE;
    }

    AdwTabView *view = kosmos_main_window_pane_view_for(self, pane_id);
    if (view == NULL) {
        return FALSE;
    }

    return kosmos_tabbed_pane_update_from_pane_view(self, view, pane, workspace_id, active_pane_id, TRUE);
}

GtkWidget *kosmos_tabbed_pane_create(KosmosMainWindow *self, JsonObject *pane, guint64 workspace_id, gboolean is_active_pane) {
    guint64 pane_id = 0;
    guint64 active_tab_id = 0;
    JsonArray *tabs = kosmos_json_get_array_member(pane, "tabs");

    if (!kosmos_json_get_uint_member(pane, "id", &pane_id) ||
        !kosmos_json_get_uint_member(pane, "activeTabId", &active_tab_id) ||
        tabs == NULL) {
        return kosmos_main_window_create_label("Invalid pane snapshot.", "error");
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
        if (!kosmos_json_get_uint_member(tab, "id", &tab_id) || !kosmos_json_get_string_member(tab, "title", &title)) {
            continue;
        }
        kosmos_json_get_string_member(tab, "kind", &kind);

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

    kosmos_main_window_register_pane_view(self, pane_id, tab_view);
    kosmos_main_window_set_uint64_data(G_OBJECT(container), "pane-id", pane_id);
    g_object_set_data(G_OBJECT(container), "tab-view", tab_view);
    g_object_set_data(G_OBJECT(tab_view), "pane-container", container);
    g_object_set_data(G_OBJECT(tab_view), "tab-bar", GTK_WIDGET(tab_bar));
    g_object_set_data(G_OBJECT(tab_view), "new-tab-button", new_tab_button);

    add_tab_bar_activation_controller(self, GTK_WIDGET(tab_bar), tab_view);
    kosmos_pane_dnd_configure_single_tab(
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
    gtk_box_append(GTK_BOX(container), kosmos_pane_dnd_create_split_overlay(self, tab_view, pane_id));
    g_signal_connect(tab_view, "notify::selected-page", G_CALLBACK(activate_selected_tab), self);
    g_signal_connect(tab_view, "notify::is-transferring-page", G_CALLBACK(tab_transfer_changed), NULL);
    g_signal_connect(tab_view, "close-page", G_CALLBACK(close_tab), self);
    g_signal_connect(tab_view, "create-window", G_CALLBACK(kosmos_pane_dnd_create_split_sink_for_detached_tab), self);
    g_signal_connect(tab_view, "page-detached", G_CALLBACK(tab_detached), NULL);
    g_signal_connect(tab_view, "page-reordered", G_CALLBACK(reorder_tab), self);
    g_signal_connect(new_tab_button, "clicked", G_CALLBACK(open_blank_tab), self);

    return container;
}
