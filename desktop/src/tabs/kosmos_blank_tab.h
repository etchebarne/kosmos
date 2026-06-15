#pragma once

#include <gtk/gtk.h>

#include "ipc/kosmos_ipc_protocol.h"

G_BEGIN_DECLS

typedef void (*KosmosBlankTabKindSelectedFunc)(KosmosIpcTabKind kind, gpointer user_data);

GtkWidget *kosmos_blank_tab_create(KosmosBlankTabKindSelectedFunc kind_selected, gpointer user_data);

G_END_DECLS
