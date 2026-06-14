#include "app/kosmos_application.h"

int main(int argc, char *argv[]) {
    KosmosApplication *app = kosmos_application_new();
    int status = g_application_run(G_APPLICATION(app), argc, argv);
    g_object_unref(app);

    return status;
}
