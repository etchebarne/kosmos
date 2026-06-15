#include "tabs/kosmos_blank_tab.h"

#include <adwaita.h>

typedef struct {
    KosmosIpcTabKind kind;
    KosmosBlankTabKindSelectedFunc kind_selected;
    gpointer user_data;
} TabKindSelection;

typedef struct {
    const char *label;
    KosmosIpcTabKind kind;
} TabKindButton;

static void tab_kind_selection_free(TabKindSelection *selection) {
    g_free(selection);
}

static void tab_kind_selection_closure_free(gpointer data, GClosure *closure) {
    (void)closure;

    tab_kind_selection_free(data);
}

static void select_tab_kind(GtkButton *button, gpointer user_data) {
    (void)button;

    TabKindSelection *selection = user_data;
    if (selection->kind_selected != NULL) {
        selection->kind_selected(selection->kind, selection->user_data);
    }
}

static GtkWidget *create_tab_kind_button(
    const TabKindButton *tab_kind,
    KosmosBlankTabKindSelectedFunc kind_selected,
    gpointer user_data
) {
    GtkWidget *button = gtk_button_new_with_label(tab_kind->label);
    TabKindSelection *selection = g_new(TabKindSelection, 1);

    selection->kind = tab_kind->kind;
    selection->kind_selected = kind_selected;
    selection->user_data = user_data;

    g_signal_connect_data(
        button,
        "clicked",
        G_CALLBACK(select_tab_kind),
        selection,
        tab_kind_selection_closure_free,
        0
    );

    return button;
}

GtkWidget *kosmos_blank_tab_create(KosmosBlankTabKindSelectedFunc kind_selected, gpointer user_data) {
    static const TabKindButton tab_kinds[] = {
        {"File Tree", KOSMOS_IPC_TAB_KIND_FILE_TREE},
        {"Editor", KOSMOS_IPC_TAB_KIND_EDITOR},
        {"Git", KOSMOS_IPC_TAB_KIND_GIT},
        {"Search", KOSMOS_IPC_TAB_KIND_SEARCH},
        {"Terminal", KOSMOS_IPC_TAB_KIND_TERMINAL},
        {"Settings", KOSMOS_IPC_TAB_KIND_SETTINGS},
    };

    GtkWidget *content = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    GtkWidget *top_spacer = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    GtkWidget *list = gtk_box_new(GTK_ORIENTATION_VERTICAL, 12);
    GtkWidget *buttons = adw_wrap_box_new();
    GtkWidget *bottom_spacer = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    GtkWidget *label = gtk_label_new("Choose a tab type");

    gtk_widget_set_hexpand(content, TRUE);
    gtk_widget_set_vexpand(content, TRUE);
    gtk_widget_set_margin_top(content, 24);
    gtk_widget_set_margin_bottom(content, 24);
    gtk_widget_set_margin_start(content, 24);
    gtk_widget_set_margin_end(content, 24);
    gtk_widget_set_vexpand(top_spacer, TRUE);
    gtk_widget_set_vexpand(bottom_spacer, TRUE);
    gtk_widget_set_halign(list, GTK_ALIGN_FILL);
    gtk_widget_set_hexpand(list, TRUE);
    gtk_widget_set_halign(label, GTK_ALIGN_CENTER);
    gtk_widget_set_halign(buttons, GTK_ALIGN_FILL);
    gtk_widget_set_hexpand(buttons, TRUE);
    adw_wrap_box_set_child_spacing(ADW_WRAP_BOX(buttons), 8);
    adw_wrap_box_set_line_spacing(ADW_WRAP_BOX(buttons), 8);
    adw_wrap_box_set_align(ADW_WRAP_BOX(buttons), 0.5f);
    adw_wrap_box_set_wrap_policy(ADW_WRAP_BOX(buttons), ADW_WRAP_MINIMUM);
    gtk_widget_add_css_class(label, "heading");

    gtk_box_append(GTK_BOX(list), label);
    for (gsize index = 0; index < G_N_ELEMENTS(tab_kinds); index++) {
        adw_wrap_box_append(ADW_WRAP_BOX(buttons), create_tab_kind_button(&tab_kinds[index], kind_selected, user_data));
    }
    gtk_box_append(GTK_BOX(list), buttons);

    gtk_box_append(GTK_BOX(content), top_spacer);
    gtk_box_append(GTK_BOX(content), list);
    gtk_box_append(GTK_BOX(content), bottom_spacer);

    return content;
}
