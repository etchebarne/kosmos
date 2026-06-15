#pragma once

#include <gtk/gtk.h>

#include "ipc/kosmos_ipc_client.h"

G_BEGIN_DECLS

GtkWidget *kosmos_file_tree_tab_create(KosmosIpcClient *ipc_client, guint64 workspace_id);

G_END_DECLS
