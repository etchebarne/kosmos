project(
  'kosmos-desktop',
  'c',
  version: '0.1.0',
  default_options: [
    'c_std=c17',
    'warning_level=2',
    'werror=false',
  ],
)

gtk_dep = dependency('gtk4')
gio_unix_dep = dependency('gio-unix-2.0')
json_glib_dep = dependency('json-glib-1.0')
libadwaita_dep = dependency('libadwaita-1')

sources = files(
  'src/main.c',
  'src/app/kosmos_application.c',
  'src/ipc/kosmos_ipc_client.c',
  'src/ipc/kosmos_ipc_pane.c',
  'src/ipc/kosmos_ipc_protocol.c',
  'src/ipc/kosmos_ipc_tab.c',
  'src/ipc/kosmos_ipc_workspace.c',
  'src/tabs/kosmos_blank_tab.c',
  'src/ui/kosmos_main_window.c',
  'src/ui/kosmos_pane_dnd.c',
  'src/ui/kosmos_pane_layout.c',
  'src/ui/kosmos_tabbed_pane.c',
  'src/ui/kosmos_workspace_switcher.c',
)

executable(
  'kosmos-desktop',
  sources,
  include_directories: include_directories('src'),
  dependencies: [gtk_dep, gio_unix_dep, json_glib_dep, libadwaita_dep],
  install: true,
)
