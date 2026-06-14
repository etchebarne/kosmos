#pragma once

#include <gtk/gtk.h>

G_BEGIN_DECLS

#define KOSMOS_TYPE_APPLICATION (kosmos_application_get_type())

G_DECLARE_FINAL_TYPE(KosmosApplication, kosmos_application, KOSMOS, APPLICATION, GtkApplication)

KosmosApplication *kosmos_application_new(void);

G_END_DECLS
