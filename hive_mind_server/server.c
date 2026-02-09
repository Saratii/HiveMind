#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#include <uv.h>

static uv_loop_t *event_loop;

typedef struct
{
    uv_tcp_t tcp_handle;
} server_client_t;

static void on_client_closed(uv_handle_t *handle)
{
    free(handle);
}

static void on_write_complete(uv_write_t *write_request, int status)
{
    if (status < 0)
    {
    }

    server_client_t *client =
        (server_client_t *)write_request->handle;

    uv_close((uv_handle_t *)client, on_client_closed);

    free(write_request);
}

static void allocate_buffer(
    uv_handle_t *handle,
    size_t suggested_size,
    uv_buf_t *buffer)
{
    buffer->base = malloc(suggested_size);
    buffer->len = suggested_size;
}

static void on_client_read(
    uv_stream_t *stream,
    ssize_t bytes_read,
    const uv_buf_t *buffer)
{
    if (bytes_read > 0)
    {
        const char *response_text =
            "HTTP/1.1 200 OK\r\n"
            "Content-Type: text/plain\r\n"
            "Content-Length: 12\r\n"
            "\r\n"
            "hello world";

        size_t response_length = strlen(response_text);

        char *response_memory = malloc(response_length);
        memcpy(response_memory, response_text, response_length);

        uv_buf_t response_buffer =
            uv_buf_init(response_memory, (unsigned int)response_length);

        uv_write_t *write_request =
            malloc(sizeof(uv_write_t));

        uv_write(
            write_request,
            stream,
            &response_buffer,
            1,
            on_write_complete);
    }
    else
    {
        if (bytes_read < 0)
        {
            uv_close((uv_handle_t *)stream, on_client_closed);
        }
    }

    if (buffer->base != NULL)
    {
        free(buffer->base);
    }
}

static void on_new_connection(
    uv_stream_t *server,
    int status)
{
    if (status < 0)
    {
        return;
    }

    server_client_t *client =
        malloc(sizeof(server_client_t));

    uv_tcp_init(event_loop, &client->tcp_handle);

    int accept_result =
        uv_accept(server, (uv_stream_t *)&client->tcp_handle);

    if (accept_result == 0)
    {
        uv_read_start(
            (uv_stream_t *)&client->tcp_handle,
            allocate_buffer,
            on_client_read);
    }
    else
    {
        uv_close((uv_handle_t *)&client->tcp_handle, on_client_closed);
    }
}

int main(void)
{
    event_loop = uv_default_loop();

    uv_tcp_t server_handle;

    uv_tcp_init(event_loop, &server_handle);

    struct sockaddr_in server_address;

    uv_ip4_addr("0.0.0.0", 8080, &server_address);

    uv_tcp_bind(
        &server_handle,
        (const struct sockaddr *)&server_address,
        0);

    int listen_result =
        uv_listen(
            (uv_stream_t *)&server_handle,
            512,
            on_new_connection);

    if (listen_result != 0)
    {
        fprintf(stderr, "listen failed\n");
        return EXIT_FAILURE;
    }

    uv_run(event_loop, UV_RUN_DEFAULT);

    return EXIT_SUCCESS;
}
