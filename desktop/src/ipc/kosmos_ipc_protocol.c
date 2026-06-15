#include "ipc/kosmos_ipc_protocol.h"

const char *kosmos_ipc_domain_to_string(KosmosIpcDomain domain) {
    switch (domain) {
    case KOSMOS_IPC_DOMAIN_WORKSPACE:
        return "workspace";
    case KOSMOS_IPC_DOMAIN_PANE:
        return "pane";
    case KOSMOS_IPC_DOMAIN_TAB:
        return "tab";
    case KOSMOS_IPC_DOMAIN_FILE_TREE:
        return "fileTree";
    }

    g_return_val_if_reached("workspace");
}

const char *kosmos_ipc_split_axis_to_string(KosmosIpcSplitAxis axis) {
    switch (axis) {
    case KOSMOS_IPC_SPLIT_AXIS_HORIZONTAL:
        return "horizontal";
    case KOSMOS_IPC_SPLIT_AXIS_VERTICAL:
        return "vertical";
    }

    g_return_val_if_reached("horizontal");
}

const char *kosmos_ipc_tab_kind_to_string(KosmosIpcTabKind kind) {
    switch (kind) {
    case KOSMOS_IPC_TAB_KIND_BLANK:
        return "blank";
    case KOSMOS_IPC_TAB_KIND_FILE_TREE:
        return "fileTree";
    case KOSMOS_IPC_TAB_KIND_EDITOR:
        return "editor";
    case KOSMOS_IPC_TAB_KIND_GIT:
        return "git";
    case KOSMOS_IPC_TAB_KIND_SEARCH:
        return "search";
    case KOSMOS_IPC_TAB_KIND_TERMINAL:
        return "terminal";
    case KOSMOS_IPC_TAB_KIND_SETTINGS:
        return "settings";
    }

    g_return_val_if_reached("blank");
}
