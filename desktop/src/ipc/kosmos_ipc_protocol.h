#pragma once

#include <glib.h>

G_BEGIN_DECLS

typedef enum {
    KOSMOS_IPC_DOMAIN_WORKSPACE,
    KOSMOS_IPC_DOMAIN_PANE,
    KOSMOS_IPC_DOMAIN_TAB,
    KOSMOS_IPC_DOMAIN_FILE_TREE,
} KosmosIpcDomain;

typedef enum {
    KOSMOS_IPC_SPLIT_AXIS_HORIZONTAL,
    KOSMOS_IPC_SPLIT_AXIS_VERTICAL,
} KosmosIpcSplitAxis;

typedef enum {
    KOSMOS_IPC_TAB_KIND_BLANK,
    KOSMOS_IPC_TAB_KIND_FILE_TREE,
    KOSMOS_IPC_TAB_KIND_EDITOR,
    KOSMOS_IPC_TAB_KIND_GIT,
    KOSMOS_IPC_TAB_KIND_SEARCH,
    KOSMOS_IPC_TAB_KIND_TERMINAL,
    KOSMOS_IPC_TAB_KIND_SETTINGS,
} KosmosIpcTabKind;

#define KOSMOS_IPC_ACTION_LIST "list"
#define KOSMOS_IPC_ACTION_CREATE "create"
#define KOSMOS_IPC_ACTION_RENAME "rename"
#define KOSMOS_IPC_ACTION_DELETE "delete"
#define KOSMOS_IPC_ACTION_OPEN "open"
#define KOSMOS_IPC_ACTION_ACTIVATE "activate"
#define KOSMOS_IPC_ACTION_SET_KIND "setKind"
#define KOSMOS_IPC_ACTION_CLOSE "close"
#define KOSMOS_IPC_ACTION_SPLIT "split"
#define KOSMOS_IPC_ACTION_REORDER "reorder"
#define KOSMOS_IPC_ACTION_MOVE "move"
#define KOSMOS_IPC_ACTION_COPY "copy"
#define KOSMOS_IPC_ACTION_RESIZE "resize"

const char *kosmos_ipc_domain_to_string(KosmosIpcDomain domain);
const char *kosmos_ipc_split_axis_to_string(KosmosIpcSplitAxis axis);
const char *kosmos_ipc_tab_kind_to_string(KosmosIpcTabKind kind);

G_END_DECLS
