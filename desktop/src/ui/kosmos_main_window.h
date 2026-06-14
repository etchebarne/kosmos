#pragma once

#include <gtk/gtk.h>

#include "ipc/kosmos_ipc_client.h"

G_BEGIN_DECLS

#define KOSMOS_TYPE_MAIN_WINDOW (kosmos_main_window_get_type())

G_DECLARE_FINAL_TYPE(KosmosMainWindow, kosmos_main_window, KOSMOS, MAIN_WINDOW, GtkApplicationWindow)

GtkWidget *kosmos_main_window_new(GtkApplication *application, KosmosIpcClient *ipc_client);
void kosmos_main_window_refresh_workspace_state(KosmosMainWindow *self);

G_END_DECLS
