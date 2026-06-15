#pragma once

#include <adwaita.h>
#include <json-glib/json-glib.h>

#include "ui/kosmos_main_window.h"

#define KOSMOS_MIN_PANE_WIDTH 220
#define KOSMOS_MIN_PANE_HEIGHT 160

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
    guint64 target_pane_id;
    KosmosIpcSplitAxis axis;
    gboolean new_pane_first;
} KosmosSplitDropZone;

GtkWidget *kosmos_main_window_create_label(const char *text, const char *css_class);
void kosmos_main_window_clear_content_area(KosmosMainWindow *self);

void kosmos_main_window_set_uint64_data(GObject *object, const char *key, guint64 value);
gboolean kosmos_main_window_get_uint64_data(GObject *object, const char *key, guint64 *value);

void kosmos_main_window_register_pane_view(KosmosMainWindow *self, guint64 pane_id, AdwTabView *view);
AdwTabView *kosmos_main_window_pane_view_for(KosmosMainWindow *self, guint64 pane_id);

gboolean kosmos_json_get_uint_member(JsonObject *object, const char *name, guint64 *value);
gboolean kosmos_json_get_string_member(JsonObject *object, const char *name, const char **value);
gboolean kosmos_json_get_double_member(JsonObject *object, const char *name, double *value);
JsonObject *kosmos_json_get_object_member(JsonObject *object, const char *name);
JsonArray *kosmos_json_get_array_member(JsonObject *object, const char *name);

void kosmos_main_window_set_status(KosmosMainWindow *self, const char *status);
void kosmos_main_window_set_error_status(KosmosMainWindow *self, const char *prefix, GError *error);
gboolean kosmos_main_window_ensure_connected(KosmosMainWindow *self);
void kosmos_main_window_apply_server_state_or_show_error(
    KosmosMainWindow *self,
    JsonNode *state,
    GError *error,
    const char *error_prefix
);
void kosmos_main_window_apply_workspace_state(KosmosMainWindow *self, JsonNode *state);

GtkWidget *kosmos_workspace_switcher_create(KosmosMainWindow *self);
GtkWidget *kosmos_workspace_switcher_ensure_add_button(KosmosMainWindow *self, gboolean sensitive);
void kosmos_workspace_switcher_clear(KosmosMainWindow *self);

char *kosmos_pane_layout_create_signature(JsonObject *workspace);
gboolean kosmos_pane_layout_update_active_workspace_in_place(KosmosMainWindow *self, JsonObject *workspace);
void kosmos_pane_layout_render_active_workspace(KosmosMainWindow *self, JsonObject *workspace);

GtkWidget *kosmos_tabbed_pane_create(
    KosmosMainWindow *self,
    JsonObject *pane,
    guint64 workspace_id,
    gboolean is_active_pane
);
gboolean kosmos_tabbed_pane_update_from_pane(
    KosmosMainWindow *self,
    JsonObject *pane,
    guint64 workspace_id,
    guint64 active_pane_id
);
gboolean kosmos_tabbed_pane_update_from_pane_view(
    KosmosMainWindow *self,
    AdwTabView *view,
    JsonObject *pane,
    guint64 workspace_id,
    guint64 active_pane_id,
    gboolean allow_append
);
void kosmos_tabbed_pane_clear_pending_activation(AdwTabView *view);

void kosmos_pane_dnd_install_css(GtkWidget *widget);
GtkWidget *kosmos_pane_dnd_create_split_overlay(KosmosMainWindow *self, AdwTabView *tab_view, guint64 pane_id);
AdwTabView *kosmos_pane_dnd_create_split_sink_for_detached_tab(AdwTabView *view, gpointer user_data);
void kosmos_pane_dnd_clear_detached_tab_transfer(KosmosMainWindow *self);
void kosmos_pane_dnd_configure_single_tab(
    KosmosMainWindow *self,
    GtkWidget *tab_bar,
    GtkWidget *new_tab_button,
    AdwTabView *tab_view,
    guint64 workspace_id,
    guint64 pane_id,
    int tab_count,
    AdwTabPage *active_page
);
