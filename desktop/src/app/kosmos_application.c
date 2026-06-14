#include "app/kosmos_application.h"

#include "ipc/kosmos_ipc_client.h"
#include "ui/kosmos_main_window.h"

struct _KosmosApplication {
    GtkApplication parent_instance;
    KosmosIpcClient *ipc_client;
};

G_DEFINE_FINAL_TYPE(KosmosApplication, kosmos_application, GTK_TYPE_APPLICATION)

static void kosmos_application_activate(GApplication *application) {
    GtkWindow *active_window = gtk_application_get_active_window(GTK_APPLICATION(application));
    if (active_window != NULL) {
        gtk_window_present(active_window);
        return;
    }

    KosmosApplication *self = KOSMOS_APPLICATION(application);
    GtkWidget *window = kosmos_main_window_new(GTK_APPLICATION(application), self->ipc_client);
    kosmos_main_window_refresh_workspace_state(KOSMOS_MAIN_WINDOW(window));
    gtk_window_present(GTK_WINDOW(window));
}

static void kosmos_application_finalize(GObject *object) {
    KosmosApplication *self = KOSMOS_APPLICATION(object);

    g_clear_object(&self->ipc_client);

    G_OBJECT_CLASS(kosmos_application_parent_class)->finalize(object);
}

static void kosmos_application_class_init(KosmosApplicationClass *klass) {
    GApplicationClass *application_class = G_APPLICATION_CLASS(klass);
    application_class->activate = kosmos_application_activate;

    GObjectClass *object_class = G_OBJECT_CLASS(klass);
    object_class->finalize = kosmos_application_finalize;
}

static void kosmos_application_init(KosmosApplication *self) {
    self->ipc_client = kosmos_ipc_client_new_from_environment();
}

KosmosApplication *kosmos_application_new(void) {
    return g_object_new(
        KOSMOS_TYPE_APPLICATION,
        "application-id", "dev.kosmos.Editor",
        "flags", (GApplicationFlags)0,
        NULL
    );
}
