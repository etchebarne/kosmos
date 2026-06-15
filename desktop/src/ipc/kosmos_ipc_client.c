#include "ipc/kosmos_ipc_client.h"

#include <gio/gunixsocketaddress.h>
#include <string.h>

struct _KosmosIpcClient {
    GObject parent_instance;
    char *socket_path;
    GSocketConnection *connection;
    GDataInputStream *input;
    GOutputStream *output;
    guint64 next_request_id;
};

G_DEFINE_FINAL_TYPE(KosmosIpcClient, kosmos_ipc_client, G_TYPE_OBJECT)
G_DEFINE_QUARK(kosmos-ipc-error-quark, kosmos_ipc_error)

static gboolean set_not_connected_error(KosmosIpcClient *self, GError **error) {
    g_set_error(
        error,
        KOSMOS_IPC_ERROR,
        KOSMOS_IPC_ERROR_NOT_CONNECTED,
        "IPC client is not connected to %s",
        self->socket_path
    );
    return FALSE;
}

static JsonNode *params_to_node(JsonObject *params) {
    JsonNode *node = json_node_new(JSON_NODE_OBJECT);
    json_node_set_object(node, params);
    return node;
}

static char *serialize_request(guint64 id, KosmosIpcDomain domain, const char *action, JsonObject *params) {
    JsonObject *empty_params = NULL;
    if (params == NULL) {
        empty_params = json_object_new();
        params = empty_params;
    }

    JsonBuilder *builder = json_builder_new();
    json_builder_begin_object(builder);

    json_builder_set_member_name(builder, "type");
    json_builder_add_string_value(builder, "request");

    json_builder_set_member_name(builder, "id");
    json_builder_add_int_value(builder, (gint64)id);

    json_builder_set_member_name(builder, "domain");
    json_builder_add_string_value(builder, kosmos_ipc_domain_to_string(domain));

    json_builder_set_member_name(builder, "action");
    json_builder_add_string_value(builder, action);

    json_builder_set_member_name(builder, "params");
    json_builder_add_value(builder, params_to_node(params));

    json_builder_end_object(builder);

    JsonNode *root = json_builder_get_root(builder);
    JsonGenerator *generator = json_generator_new();
    json_generator_set_root(generator, root);
    char *payload = json_generator_to_data(generator, NULL);

    json_node_unref(root);
    g_object_unref(generator);
    g_object_unref(builder);
    g_clear_pointer(&empty_params, json_object_unref);

    return payload;
}

static gboolean write_frame(KosmosIpcClient *self, const char *payload, GCancellable *cancellable, GError **error) {
    char *frame = g_strconcat(payload, "\n", NULL);
    gboolean written = g_output_stream_write_all(
        self->output,
        frame,
        strlen(frame),
        NULL,
        cancellable,
        error
    );

    g_free(frame);

    if (!written || !g_output_stream_flush(self->output, cancellable, error)) {
        kosmos_ipc_client_disconnect(self);
        return FALSE;
    }

    return TRUE;
}

static char *read_frame(KosmosIpcClient *self, GCancellable *cancellable, GError **error) {
    gsize length = 0;
    char *line = g_data_input_stream_read_line_utf8(self->input, &length, cancellable, error);

    if (line == NULL && (error == NULL || *error == NULL)) {
        g_set_error(
            error,
            KOSMOS_IPC_ERROR,
            KOSMOS_IPC_ERROR_IO,
            "IPC server closed the connection"
        );
    }

    if (line == NULL) {
        kosmos_ipc_client_disconnect(self);
    }

    return line;
}

static gboolean get_string_member(JsonObject *object, const char *name, const char **value) {
    JsonNode *node = json_object_get_member(object, name);
    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node) || json_node_get_value_type(node) != G_TYPE_STRING) {
        return FALSE;
    }

    *value = json_node_get_string(node);
    return TRUE;
}

static gboolean get_uint_member(JsonObject *object, const char *name, guint64 *value) {
    JsonNode *node = json_object_get_member(object, name);
    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node)) {
        return FALSE;
    }

    gint64 signed_value = json_node_get_int(node);
    if (signed_value < 0) {
        return FALSE;
    }

    *value = (guint64)signed_value;
    return TRUE;
}

static gboolean get_bool_member(JsonObject *object, const char *name, gboolean *value) {
    JsonNode *node = json_object_get_member(object, name);
    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node) || json_node_get_value_type(node) != G_TYPE_BOOLEAN) {
        return FALSE;
    }

    *value = json_node_get_boolean(node);
    return TRUE;
}

static gboolean set_invalid_response_error(const char *reason, GError **error) {
    g_set_error(
        error,
        KOSMOS_IPC_ERROR,
        KOSMOS_IPC_ERROR_INVALID_RESPONSE,
        "Invalid IPC response: %s",
        reason
    );
    return FALSE;
}

static gboolean set_server_response_error(JsonObject *response, GError **error) {
    const char *code = "ipc.server_error";
    const char *message = "server returned an IPC error";

    JsonNode *error_node = json_object_get_member(response, "error");
    if (error_node != NULL && JSON_NODE_HOLDS_OBJECT(error_node)) {
        JsonObject *server_error = json_node_get_object(error_node);
        get_string_member(server_error, "code", &code);
        get_string_member(server_error, "message", &message);
    }

    g_set_error(
        error,
        KOSMOS_IPC_ERROR,
        KOSMOS_IPC_ERROR_SERVER,
        "%s: %s",
        code,
        message
    );

    return FALSE;
}

static gboolean parse_response(const char *payload, guint64 request_id, JsonNode **result, GError **error) {
    JsonParser *parser = json_parser_new();
    GError *parse_error = NULL;

    if (!json_parser_load_from_data(parser, payload, -1, &parse_error)) {
        g_set_error(
            error,
            KOSMOS_IPC_ERROR,
            KOSMOS_IPC_ERROR_INVALID_RESPONSE,
            "Invalid IPC response JSON: %s",
            parse_error->message
        );
        g_clear_error(&parse_error);
        g_object_unref(parser);
        return FALSE;
    }

    JsonNode *root = json_parser_get_root(parser);
    if (root == NULL || !JSON_NODE_HOLDS_OBJECT(root)) {
        g_object_unref(parser);
        return set_invalid_response_error("response root is not an object", error);
    }

    JsonObject *response = json_node_get_object(root);
    const char *type = NULL;
    guint64 response_id = 0;
    gboolean ok = FALSE;

    if (!get_string_member(response, "type", &type) || g_strcmp0(type, "response") != 0) {
        g_object_unref(parser);
        return set_invalid_response_error("missing response type", error);
    }

    if (!get_uint_member(response, "id", &response_id) || response_id != request_id) {
        g_object_unref(parser);
        return set_invalid_response_error("response id does not match request id", error);
    }

    if (!get_bool_member(response, "ok", &ok)) {
        g_object_unref(parser);
        return set_invalid_response_error("missing ok flag", error);
    }

    if (!ok) {
        gboolean server_error = set_server_response_error(response, error);
        g_object_unref(parser);
        return server_error;
    }

    if (result != NULL) {
        JsonNode *result_node = json_object_get_member(response, "result");
        *result = result_node == NULL ? NULL : json_node_copy(result_node);
    }

    g_object_unref(parser);
    return TRUE;
}

static void kosmos_ipc_client_finalize(GObject *object) {
    KosmosIpcClient *self = KOSMOS_IPC_CLIENT(object);

    kosmos_ipc_client_disconnect(self);
    g_clear_pointer(&self->socket_path, g_free);

    G_OBJECT_CLASS(kosmos_ipc_client_parent_class)->finalize(object);
}

static void kosmos_ipc_client_class_init(KosmosIpcClientClass *klass) {
    GObjectClass *object_class = G_OBJECT_CLASS(klass);
    object_class->finalize = kosmos_ipc_client_finalize;
}

static void kosmos_ipc_client_init(KosmosIpcClient *self) {
    self->next_request_id = 1;
}

char *kosmos_ipc_default_socket_path(void) {
    const char *socket_path = g_getenv("KOSMOS_SOCKET");
    if (socket_path != NULL && socket_path[0] != '\0') {
        return g_strdup(socket_path);
    }

    const char *runtime_dir = g_getenv("XDG_RUNTIME_DIR");
    if (runtime_dir == NULL || runtime_dir[0] == '\0') {
        runtime_dir = g_get_tmp_dir();
    }

    return g_build_filename(runtime_dir, "kosmos", "server.sock", NULL);
}

KosmosIpcClient *kosmos_ipc_client_new(const char *socket_path) {
    KosmosIpcClient *self = g_object_new(KOSMOS_TYPE_IPC_CLIENT, NULL);
    self->socket_path = socket_path == NULL || socket_path[0] == '\0'
        ? kosmos_ipc_default_socket_path()
        : g_strdup(socket_path);

    return self;
}

KosmosIpcClient *kosmos_ipc_client_new_from_environment(void) {
    return kosmos_ipc_client_new(NULL);
}

const char *kosmos_ipc_client_get_socket_path(KosmosIpcClient *self) {
    g_return_val_if_fail(KOSMOS_IS_IPC_CLIENT(self), NULL);

    return self->socket_path;
}

gboolean kosmos_ipc_client_is_connected(KosmosIpcClient *self) {
    g_return_val_if_fail(KOSMOS_IS_IPC_CLIENT(self), FALSE);

    return self->connection != NULL && !g_io_stream_is_closed(G_IO_STREAM(self->connection));
}

gboolean kosmos_ipc_client_connect(KosmosIpcClient *self, GCancellable *cancellable, GError **error) {
    g_return_val_if_fail(KOSMOS_IS_IPC_CLIENT(self), FALSE);

    if (kosmos_ipc_client_is_connected(self)) {
        return TRUE;
    }

    GSocket *socket = g_socket_new(
        G_SOCKET_FAMILY_UNIX,
        G_SOCKET_TYPE_STREAM,
        G_SOCKET_PROTOCOL_DEFAULT,
        error
    );

    if (socket == NULL) {
        return FALSE;
    }

    GSocketAddress *address = g_unix_socket_address_new(self->socket_path);
    if (!g_socket_connect(socket, address, cancellable, error)) {
        g_object_unref(address);
        g_object_unref(socket);
        return FALSE;
    }

    self->connection = g_socket_connection_factory_create_connection(socket);
    g_object_unref(address);
    g_object_unref(socket);

    if (self->connection == NULL) {
        g_set_error(
            error,
            KOSMOS_IPC_ERROR,
            KOSMOS_IPC_ERROR_CONNECTION,
            "failed to create IPC socket connection"
        );
        return FALSE;
    }

    self->input = g_data_input_stream_new(g_io_stream_get_input_stream(G_IO_STREAM(self->connection)));
    g_data_input_stream_set_newline_type(self->input, G_DATA_STREAM_NEWLINE_TYPE_LF);
    self->output = g_object_ref(g_io_stream_get_output_stream(G_IO_STREAM(self->connection)));

    return TRUE;
}

void kosmos_ipc_client_disconnect(KosmosIpcClient *self) {
    g_return_if_fail(KOSMOS_IS_IPC_CLIENT(self));

    if (self->connection != NULL && !g_io_stream_is_closed(G_IO_STREAM(self->connection))) {
        g_io_stream_close(G_IO_STREAM(self->connection), NULL, NULL);
    }

    g_clear_object(&self->input);
    g_clear_object(&self->output);
    g_clear_object(&self->connection);
}

gboolean kosmos_ipc_client_request(
    KosmosIpcClient *self,
    KosmosIpcDomain domain,
    const char *action,
    JsonObject *params,
    JsonNode **result,
    GCancellable *cancellable,
    GError **error
) {
    g_return_val_if_fail(KOSMOS_IS_IPC_CLIENT(self), FALSE);
    g_return_val_if_fail(action != NULL, FALSE);

    if (result != NULL) {
        *result = NULL;
    }

    if (!kosmos_ipc_client_is_connected(self)) {
        return set_not_connected_error(self, error);
    }

    guint64 request_id = self->next_request_id++;
    char *payload = serialize_request(request_id, domain, action, params);

    if (!write_frame(self, payload, cancellable, error)) {
        g_free(payload);
        return FALSE;
    }

    g_free(payload);

    char *response = read_frame(self, cancellable, error);
    if (response == NULL) {
        return FALSE;
    }

    gboolean parsed = parse_response(response, request_id, result, error);
    g_free(response);

    return parsed;
}
