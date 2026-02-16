#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <math.h>
#include "car.h"

#include <uv.h>

static uv_loop_t *event_loop;

typedef struct
{
    uv_tcp_t tcp_handle;
} server_client_t;

typedef struct
{
    uv_tcp_t tcp_handle;
    uv_connect_t connect_req;
    char request_data[512];
} http_client_t;

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

static int starts_with(const char *str, const char *prefix)
{
    return strncmp(str, prefix, strlen(prefix)) == 0;
}

static void on_car_server_closed(uv_handle_t *handle)
{
    http_client_t *client = (http_client_t *)handle;
    free(client);
}

static void on_car_server_write_complete(uv_write_t *req, int status)
{
    if (status < 0)
    {
    }

    http_client_t *client = (http_client_t *)req->data;
    uv_close((uv_handle_t *)&client->tcp_handle, on_car_server_closed);
    free(req);
}

static void on_car_server_connected(uv_connect_t *req, int status)
{
    http_client_t *client = (http_client_t *)req->data;

    if (status < 0)
    {
        uv_close((uv_handle_t *)&client->tcp_handle, on_car_server_closed);
        return;
    }

    uv_buf_t write_buf = uv_buf_init(client->request_data, strlen(client->request_data));
    uv_write_t *write_req = malloc(sizeof(uv_write_t));
    write_req->data = client;

    uv_write(write_req, (uv_stream_t *)&client->tcp_handle, &write_buf, 1, on_car_server_write_complete);
}

static void send_route_to_car(const char *license, Point start, Point dest)
{
    double speed = 10.0;

    char body[256];
    int body_len = snprintf(body, sizeof(body),
                            "license=%s&speed=%.2f&start_x=%.2f&start_y=%.2f&dest_x=%.2f&dest_y=%.2f",
                            license, speed, start.x, start.y, dest.x, dest.y);

    http_client_t *client = malloc(sizeof(http_client_t));
    client->connect_req.data = client;

    uv_tcp_init(event_loop, &client->tcp_handle);

    struct sockaddr_in car_server_addr;
    uv_ip4_addr("127.0.0.1", 8081, &car_server_addr);

    snprintf(client->request_data, sizeof(client->request_data),
             "POST /set-route HTTP/1.1\r\n"
             "Host: 127.0.0.1:8081\r\n"
             "Content-Type: application/x-www-form-urlencoded\r\n"
             "Content-Length: %d\r\n"
             "\r\n"
             "%s",
             body_len, body);

    uv_tcp_connect(&client->connect_req, &client->tcp_handle,
                   (const struct sockaddr *)&car_server_addr, on_car_server_connected);
}

static void on_client_read(
    uv_stream_t *stream,
    ssize_t bytes_read,
    const uv_buf_t *buffer)
{
    if (bytes_read > 0)
    {
        const char *response_text = NULL;
        char response_buffer_text[256];

        if (starts_with(buffer->base, "POST /register-car"))
        {
            char license[20] = {0};
            Point start = {0, 0};
            Point dest = {0, 0};

            char *body = strstr(buffer->base, "\r\n\r\n");
            if (body != NULL)
            {
                body += 4;
                sscanf(body, "license=%19[^&]&start_x=%lf&start_y=%lf&dest_x=%lf&dest_y=%lf",
                       license, &start.x, &start.y, &dest.x, &dest.y);

                add_car(license, start, dest);
                printf("Car registered: %s (%.2f, %.2f) -> (%.2f, %.2f)\n", license, start.x, start.y, dest.x, dest.y);
                send_route_to_car(license, start, dest);
                snprintf(response_buffer_text, sizeof(response_buffer_text),
                         "HTTP/1.1 200 OK\r\n"
                         "Content-Type: text/plain\r\n"
                         "Content-Length: 18\r\n"
                         "\r\n"
                         "Car registered: %s",
                         license);
                response_text = response_buffer_text;
            }
        }
        else
        {
            int car_count = get_car_count();
            snprintf(response_buffer_text, sizeof(response_buffer_text),
                     "HTTP/1.1 200 OK\r\n"
                     "Content-Type: text/plain\r\n"
                     "Content-Length: 30\r\n"
                     "\r\n"
                     "Total cars registered: %d",
                     car_count);
            response_text = response_buffer_text;
        }

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
    if (listen_result != 0)
    {
        fprintf(stderr, "listen failed\n");
        return EXIT_FAILURE;
    }
    printf("Server running on http://0.0.0.0:8080\n");
    uv_run(event_loop, UV_RUN_DEFAULT);
    return EXIT_SUCCESS;
}
