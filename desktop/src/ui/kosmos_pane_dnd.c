#include "ui/kosmos_main_window_private.h"

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

static void show_pane_drag_preview(PaneDrag *drag);
static gboolean pane_drag_start_is_valid(PaneDrag *drag, double x, double y);

void kosmos_pane_dnd_install_css(GtkWidget *widget) {
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

static void detached_tab_split_free(DetachedTabSplit *split) {
    g_object_unref(split->window);
    g_free(split);
}

void kosmos_pane_dnd_clear_detached_tab_transfer(KosmosMainWindow *self) {
    self->splitting_detached_tab = FALSE;

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
    if (!kosmos_main_window_ensure_connected(self)) {
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
    kosmos_main_window_apply_server_state_or_show_error(self, state, error, "Failed to split tab");

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
    if (!kosmos_main_window_ensure_connected(self)) {
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
    kosmos_main_window_apply_server_state_or_show_error(self, state, error, "Failed to move pane");

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

    kosmos_pane_dnd_clear_detached_tab_transfer(self);
    return G_SOURCE_REMOVE;
}

static void split_attached_tab(AdwTabView *view, AdwTabPage *page, int position, gpointer user_data) {
    (void)position;

    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    if (!ADW_IS_TAB_VIEW(view) || !ADW_IS_TAB_PAGE(page)) {
        return;
    }

    gboolean is_split_target = g_object_get_data(G_OBJECT(view), "split-drop-target") != NULL;

    if (!self->splitting_detached_tab && !is_split_target) {
        return;
    }

    self->splitting_detached_tab = FALSE;

    guint64 workspace_id = 0;
    guint64 pane_id = 0;
    guint64 tab_id = 0;
    if (!kosmos_main_window_get_uint64_data(G_OBJECT(page), "workspace-id", &workspace_id) ||
        !kosmos_main_window_get_uint64_data(G_OBJECT(page), "pane-id", &pane_id) ||
        !kosmos_main_window_get_uint64_data(G_OBJECT(page), "tab-id", &tab_id)) {
        kosmos_main_window_refresh_workspace_state(self);
        return;
    }

    guint64 target_pane_id = pane_id;
    kosmos_main_window_get_uint64_data(G_OBJECT(view), "target-pane-id", &target_pane_id);

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
    g_timeout_add_full(G_PRIORITY_DEFAULT, 16, finish_detached_tab_split, split, (GDestroyNotify)detached_tab_split_free);
}

AdwTabView *kosmos_pane_dnd_create_split_sink_for_detached_tab(AdwTabView *view, gpointer user_data) {
    KosmosMainWindow *self = KOSMOS_MAIN_WINDOW(user_data);
    kosmos_tabbed_pane_clear_pending_activation(view);
    kosmos_pane_dnd_clear_detached_tab_transfer(self);

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

static void set_split_drop_target_enabled(GtkWidget *target, gboolean enabled) {
    GtkWidget *drop_receiver = g_object_get_data(G_OBJECT(target), "split-drop-receiver");
    GtkWidget *tab_view = g_object_get_data(G_OBJECT(target), "split-drop-tab-view");

    gtk_widget_set_visible(target, enabled);
    gtk_widget_set_can_target(target, enabled);
    if (drop_receiver != NULL) {
        gtk_widget_set_can_target(drop_receiver, enabled);
    }
    if (tab_view != NULL) {
        gtk_widget_set_can_target(tab_view, enabled);
    }
}

static void set_split_targets_enabled_in(GtkWidget *widget, gboolean enabled) {
    if (widget == NULL) {
        return;
    }

    if (g_object_get_data(G_OBJECT(widget), "split-drop-zone") != NULL) {
        set_split_drop_target_enabled(widget, enabled);
    }

    for (GtkWidget *child = gtk_widget_get_first_child(widget); child != NULL; child = gtk_widget_get_next_sibling(child)) {
        set_split_targets_enabled_in(child, enabled);
    }
}

void kosmos_pane_dnd_set_split_targets_enabled(KosmosMainWindow *self, gboolean enabled) {
    if (!enabled) {
        set_pane_drag_highlight(self, NULL);
    }

    set_split_targets_enabled_in(self->content_area, enabled);
    set_split_targets_enabled_in(self->staged_content_area, enabled);
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
    KosmosSplitDropZone *zone
) {
    if (!gtk_widget_get_visible(widget)) {
        return FALSE;
    }

    KosmosSplitDropZone *candidate = g_object_get_data(G_OBJECT(widget), "split-drop-zone");
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

static gboolean pane_drag_find_zone(PaneDrag *drag, double x, double y, GtkWidget **zone_widget, KosmosSplitDropZone *zone) {
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

    return picked == widget || gtk_widget_is_ancestor(picked, widget);
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
    PaneDrag *drag = user_data;
    GtkWidget *zone_widget = NULL;
    KosmosSplitDropZone zone = {0};
    double x = drag->start_x + offset_x;
    double y = drag->start_y + offset_y;

    if (!drag->active) {
        if (!pane_drag_offset_is_significant(offset_x, offset_y)) {
            return;
        }

        drag->active = TRUE;
        kosmos_tabbed_pane_clear_pending_activation(drag->view);
        g_object_set_data(G_OBJECT(drag->view), "tab-press-active", NULL);
        kosmos_pane_dnd_set_split_targets_enabled(drag->window, TRUE);
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
    KosmosSplitDropZone zone = {0};
    gboolean has_zone = pane_drag_find_zone(drag, drag->start_x + offset_x, drag->start_y + offset_y, &zone_widget, &zone);

    if (!drag->active) {
        return;
    }

    drag->active = FALSE;
    set_pane_drag_highlight(drag->window, NULL);
    clear_pane_drag_preview(drag);
    kosmos_pane_dnd_set_split_targets_enabled(drag->window, FALSE);
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

void kosmos_pane_dnd_configure_single_tab(
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
    KosmosSplitDropZone *zone = g_new(KosmosSplitDropZone, 1);

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
    kosmos_main_window_set_uint64_data(G_OBJECT(tab_view), "target-pane-id", target_pane_id);
    g_object_set_data(G_OBJECT(tab_view), "split-axis", GINT_TO_POINTER(axis + 1));
    g_object_set_data(G_OBJECT(tab_view), "split-new-pane-first", GINT_TO_POINTER((new_pane_first ? 1 : 0) + 1));
    g_signal_connect(tab_view, "page-attached", G_CALLBACK(split_attached_tab), self);

    zone->target_pane_id = target_pane_id;
    zone->axis = axis;
    zone->new_pane_first = new_pane_first;
    g_object_set_data_full(G_OBJECT(target), "split-drop-zone", zone, g_free);
    g_object_set_data(G_OBJECT(target), "split-drop-receiver", drop_receiver);
    g_object_set_data(G_OBJECT(target), "split-drop-tab-view", GTK_WIDGET(tab_view));
    g_object_set_data(G_OBJECT(target), "split-drop-highlight", highlight);

    g_object_set_data(G_OBJECT(motion), "split-drop-highlight", highlight);
    g_signal_connect(motion, "enter", G_CALLBACK(show_split_drop_highlight), NULL);
    g_signal_connect(motion, "motion", G_CALLBACK(show_split_drop_highlight), NULL);
    g_signal_connect(motion, "leave", G_CALLBACK(hide_split_drop_highlight), NULL);
    gtk_widget_add_controller(target, motion);
    set_split_drop_target_enabled(target, FALSE);

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
    set_split_drop_target_enabled(target, FALSE);
}

GtkWidget *kosmos_pane_dnd_create_split_overlay(KosmosMainWindow *self, AdwTabView *tab_view, guint64 pane_id) {
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
