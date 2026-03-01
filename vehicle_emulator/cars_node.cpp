/*
prologue
Name of program: cars_node.cpp
Description: Initialize a car structure, copies strings and sets starting positions, and velocity/acceleration. All server API and commands are handled in this file
Author: Saurav Renju / Alec Slavik
Date Created: 2/11/2026
Date Revised: 3/1/2026
Revision History: Included in the numerous sprint artifacts.
*/


// vehicle_emulator/car_node.cpp
// Sprint 1 car runtime:
// - runs a tiny HTTP server (listening endpoint) for server -> car requests
// - exposes:
//     GET  /status
//     POST /command   (form-encoded: speed=..&dest_x=..&dest_y=..)
// - registers itself with central server via:
//     POST http://localhost:8080/register-car
//     body: license=...&start_x=..&start_y=..&dest_x=..&dest_y=..
// - fixed timestep simulation loop (16ms): position += velocity * dt

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <uv.h>

#include "cars.h" // your car "class" (VIN/year/make/model + start position)

typedef struct {
    // Identity used by current server endpoint
    char license[20];

    // World-space meters
    double x, y;
    double vx, vy;

    // Destination
    double dest_x, dest_y;

    // Commanded speed (m/s)
    double target_speed;

    // Limits
    double max_accel; // m/s^2

    // Car metadata (optional)
    Car meta;
} CarRuntime;

static uv_loop_t *g_loop;

// ---------------- HTTP helpers ----------------

static int starts_with(const char *s, const char *prefix) {
    return strncmp(s, prefix, strlen(prefix)) == 0;
}

static const char* find_header(const char *req, const char *name) {
    // very simple header finder; returns pointer to header value start or NULL
    const char *p = req;
    size_t nlen = strlen(name);
    while ((p = strstr(p, name)) != NULL) {
        // ensure it matches at line start-ish
        // good enough for sprint: assume exact header present
        const char *colon = p + nlen;
        if (*colon == ':') {
            colon++;
            while (*colon == ' ') colon++;
            return colon;
        }
        p += nlen;
    }
    return NULL;
}

// Extracts the Content-Length header value from an HTTP request. Uses atoi() to convert the header value string to an integer.
static int parse_content_length(const char *req) {
    const char *v = find_header(req, "Content-Length");
    if (!v) return 0;
    return atoi(v);
}

// Locates the start of the HTTP message body. HTTP headers end with "\r\n\r\n". Returns pointer to first byte of body, or NULL if not found.
static char* find_body(char *req) {
    char *body = strstr(req, "\r\n\r\n");
    if (!body) return NULL;
    return body + 4;
}

// Builds and prepares an HTTP response header. Writes status line, content type, content length, and connection close. The actual body is sent separately after the header.
static void write_http_response(
    uv_stream_t *stream,
    const char *status_line,
    const char *content_type,
    const char *body_text
) {
    char header[512];
    int body_len = (int)strlen(body_text);

    int header_len = snprintf(
        header, sizeof(header),
        "%s\r\n"
        "Content-Type: %s\r\n"
        "Content-Length: %d\r\n"
        "Connection: close\r\n"
        "\r\n",
        status_line, content_type, body_len
    );

    // allocate one buffer holding header+body
    size_t total = (size_t)header_len + (size_t)body_len;
    char *resp = (char*)malloc(total);
    if (!resp) return;

    memcpy(resp, header, (size_t)header_len);
    memcpy(resp + header_len, body_text, (size_t)body_len);

    uv_buf_t b = uv_buf_init(resp, (unsigned int)total);

    uv_write_t *wr = (uv_write_t*)malloc(sizeof(uv_write_t));
    if (!wr) { free(resp); return; }

    // free response when write completes
    wr->data = resp;

    uv_write(wr, stream, &b, 1, [](uv_write_t *req, int status){
        (void)status;
        char *resp_mem = (char*)req->data;
        free(resp_mem);
        uv_close((uv_handle_t*)req->handle, [](uv_handle_t *h){
            free(h);
        });
        free(req);
    });
}

// ---------------- Car simulation ----------------

static void car_set_velocity_toward_dest(CarRuntime *c, double dt) {
    double dx = c->dest_x - c->x;
    double dy = c->dest_y - c->y;
    double dist = sqrt(dx*dx + dy*dy);

    if (dist < 0.5) {
        // arrived: stop
        c->vx = 0.0;
        c->vy = 0.0;
        return;
    }

    // direction to dest
    double ux = dx / dist;
    double uy = dy / dist;

    // current speed
    double speed = sqrt(c->vx*c->vx + c->vy*c->vy);

    // accelerate/decelerate toward target_speed with max_accel
    double desired = c->target_speed;
    double delta = desired - speed;
    double max_dv = c->max_accel * dt;
    if (delta >  max_dv) delta =  max_dv;
    if (delta < -max_dv) delta = -max_dv;

    double new_speed = speed + delta;
    if (new_speed < 0.0) new_speed = 0.0;

    c->vx = ux * new_speed;
    c->vy = uy * new_speed;
}

static void sim_tick(CarRuntime *c, double dt) {
    // Update velocity based on target and destination
    car_set_velocity_toward_dest(c, dt);

    // Integrate position
    c->x += c->vx * dt;
    c->y += c->vy * dt;

    // Tiny lane drift (optional, super small, keeps requirement satisfied without chaos)
    // Feel free to remove if you want dead-straight.
    // c->y += 0.02 * sin(uv_hrtime() / 1e9);
}

// ---------------- Car listening server (libuv) ----------------

// Represents a connected client.
typedef struct {
    uv_tcp_t handle;
    CarRuntime *car;
} CarClient;

// Called by libuv when memory is needed to read incoming data. Allocates a buffer of suggested size (+1 for safety/null termination).
static void alloc_buf(uv_handle_t *h, size_t suggested, uv_buf_t *buf) {
    (void)h;
    buf->base = (char*)malloc(suggested + 1);
    buf->len = suggested + 1;
}

// Called when data is received from a connected client. Handles incoming commands and client disconnects.
static void on_car_client_read(uv_stream_t *stream, ssize_t nread, const uv_buf_t *buf) {
    CarClient *cc = (CarClient*)stream;
    CarRuntime *car = cc->car;

    // EOF / error
    if (nread <= 0) {
        if (buf->base) free(buf->base);
        if (nread < 0) {
            uv_close((uv_handle_t*)stream, [](uv_handle_t *h){ free(h); });
        }
        return;
    }

    // Ensure request is null-terminated (alloc_buf gave us +1)
    buf->base[nread] = '\0';
    char *req = buf->base;

    // -------- GET /status --------
    if (starts_with(req, "GET /status")) {
        char body[256];
        snprintf(body, sizeof(body),
            "{"
              "\"license\":\"%s\","
              "\"x\":%.3f,\"y\":%.3f,"
              "\"dest_x\":%.3f,\"dest_y\":%.3f"
            "}",
            car->license, car->x, car->y, car->dest_x, car->dest_y
        );

        write_http_response(stream, "HTTP/1.1 200 OK", "application/json", body);
        free(buf->base);
        return;
    }

    // -------- POST /set-route --------
    if (starts_with(req, "POST /set-route")) {
        char *body = find_body(req);
        if (!body) {
            write_http_response(stream, "HTTP/1.1 400 Bad Request", "text/plain", "missing body");
            free(buf->base);
            return;
        }

        // Defaults (keep current unless provided)
        char license_in[32] = {0};
        double speed = car->target_speed;
        double dir_x = 0.0;
        double dir_y = 0.0;

        // Parse form fields (server sends: license, speed, direction_x, direction_y)
        if (strstr(body, "license=")) {
            // copy license value until '&' or end
            const char *p = strstr(body, "license=") + strlen("license=");
            size_t i = 0;
            while (p[i] && p[i] != '&' && i < sizeof(license_in)-1) {
                license_in[i] = p[i];
                i++;
            }
            license_in[i] = '\0';
        }

        if (strstr(body, "speed="))
            sscanf(strstr(body, "speed="), "speed=%lf", &speed);

        if (strstr(body, "direction_x="))
            sscanf(strstr(body, "direction_x="), "direction_x=%lf", &dir_x);

        if (strstr(body, "direction_y="))
            sscanf(strstr(body, "direction_y="), "direction_y=%lf", &dir_y);

        // License check (if provided)
        if (license_in[0] != '\0' && strcmp(license_in, car->license) != 0) {
            write_http_response(stream, "HTTP/1.1 404 Not Found", "text/plain", "wrong license for this car");
            free(buf->base);
            return;
        }

        // Normalize direction (avoid huge/zero vectors)
        double mag = sqrt(dir_x*dir_x + dir_y*dir_y);
        if (mag < 1e-9) {
            write_http_response(stream, "HTTP/1.1 400 Bad Request", "text/plain", "direction is zero");
            free(buf->base);
            return;
        }
        dir_x /= mag;
        dir_y /= mag;

        // Apply command:
        // - set target speed
        // - set a far-away destination in that direction so your existing sim moves
        car->target_speed = speed;
        const double LOOKAHEAD = 100.0; // meters ahead
        car->dest_x = car->x + dir_x * LOOKAHEAD;
        car->dest_y = car->y + dir_y * LOOKAHEAD;

        printf("[car %s] new route -> dir=(%.2f, %.2f) speed=%.2f\n",
               car->license, dir_x, dir_y, speed);

        write_http_response(stream, "HTTP/1.1 200 OK", "text/plain", "route updated");
        free(buf->base);
        return;
    }

    // -------- fallback --------
    write_http_response(stream, "HTTP/1.1 404 Not Found", "text/plain", "not found");
    free(buf->base);
}

// Called by libuv when a new client attempts to connect to the server.
static void on_new_car_conn(uv_stream_t *server, int status) {
    // If connection attempt failed, ignore it.
    if (status < 0) return;

    // Retrieve the underlying TCP server handle.
    uv_tcp_t *server_tcp = (uv_tcp_t*)server;
    // Access the shared CarRuntime stored in server->data.
    CarRuntime *car = (CarRuntime*)server_tcp->data;

    // Allocate memory for a new client connection wrapper.
    CarClient *client = (CarClient*)malloc(sizeof(CarClient));
    if (!client) return;

    // Associate this client with the shared car state.
    client->car = car;
    uv_tcp_init(g_loop, &client->handle);

    // Accept the incoming connection.
    if (uv_accept(server, (uv_stream_t*)&client->handle) == 0) {
        uv_read_start((uv_stream_t*)&client->handle, alloc_buf, on_car_client_read);
    } else {
        // If accept fails, close and free the client handle.
        uv_close((uv_handle_t*)&client->handle, [](uv_handle_t *h){ free(h); });
    }
}

// ---------------- Register with central server (simple libuv TCP client) ----------------
// Represents a lightweight HTTP client using libuv. Used to send a POST request to the central server.
typedef struct {
    uv_tcp_t tcp;
    uv_connect_t conn;
    uv_write_t wr;
    char *req_mem;
} HttpClient;

// Called when the TCP connection is fully closed. Frees allocated request memory and client struct.
static void on_http_client_closed(uv_handle_t *h) {
    HttpClient *hc = (HttpClient*)h->data;
    if (hc) {
        free(hc->req_mem);
        free(hc);
    }
}

// Called after the HTTP request has been written to the socket.
static void on_http_write_done(uv_write_t *wr, int status) {
    (void)status;
    // close after write; we don't need to read response for sprint 1
    uv_close((uv_handle_t*)wr->handle, on_http_client_closed);
}

// Called when the TCP connection attempt completes.
static void on_http_connected(uv_connect_t *req, int status) {
    if (status < 0) {
        fprintf(stderr, "[car_node] failed to connect to central server\n");
        HttpClient *hc = (HttpClient*)req->handle->data;
        uv_close((uv_handle_t*)&hc->tcp, on_http_client_closed);
        return;
    }

    // On successful connection, send the HTTP request.
    HttpClient *hc = (HttpClient*)req->handle->data;
    // Wrap request string in a libuv buffer
    uv_buf_t b = uv_buf_init(hc->req_mem, (unsigned int)strlen(hc->req_mem));
    // Write request to server
    uv_write(&hc->wr, req->handle, &b, 1, on_http_write_done);
}

// Sends an HTTP POST request to register this car node with the central server.
static void post_register_car(const char *host_ip, int port, CarRuntime *car, int listen_port) {
    // Build URL that points back to THIS car node
    char car_url[128];
    snprintf(car_url, sizeof(car_url), "http://127.0.0.1:%d", listen_port);

    // Build form body to match server's register_car parser:
    // license, url, start_x, start_y, dest_x, dest_y
    char body[512];
    snprintf(body, sizeof(body),
        "license=%s&url=%s&start_x=%.3f&start_y=%.3f&dest_x=%.3f&dest_y=%.3f",
        car->license, car_url, car->x, car->y, car->dest_x, car->dest_y
    );

    // Build full HTTP POST request
    char req[1024];
    int body_len = (int)strlen(body);
    snprintf(req, sizeof(req),
        "POST /register-car HTTP/1.1\r\n"
        "Host: %s:%d\r\n"
        "Content-Type: application/x-www-form-urlencoded\r\n"
        "Content-Length: %d\r\n"
        "Connection: close\r\n"
        "\r\n"
        "%s",
        host_ip, port, // Central server host/port
        body_len, // Length of form body
        body // Form data
    );

    // Allocate and zero-initialize a new HttpClient structure. calloc() ensures all fields (tcp, conn, wr, req_mem) start as NULL/0.
    HttpClient *hc = (HttpClient*)calloc(1, sizeof(HttpClient));
    if (!hc) return;

    // NOTE: _strdup is Windows-only. On mac/linux use strdup
    hc->req_mem = strdup(req);
    if (!hc->req_mem) { free(hc); return; }
    
    // Initialize the TCP handle for this HTTP client.
    uv_tcp_init(g_loop, &hc->tcp);
    // Store pointer to HttpClient inside libuv handle so we can retrieve it in callbacks.
    hc->tcp.data = hc;
    // Build destination IPv4 address structure.
    struct sockaddr_in dest;
    uv_ip4_addr(host_ip, port, &dest);
    // Initiate asynchronous TCP connection to central server. When connected, on_http_connected() will be called.
    uv_tcp_connect(&hc->conn, &hc->tcp, (const struct sockaddr*)&dest, on_http_connected);
}

// ---------------- Simulation timer ----------------
// Called every 16ms by libuv timer. Advances the physics simulation of the car.
static void on_sim_timer(uv_timer_t *t) {
    CarRuntime *car = (CarRuntime*)t->data;
    const double dt = 0.016; // 16ms

    // Advance simulation by dt seconds.
    sim_tick(car, dt);

    // optional console log every ~1s
    static int counter = 0;
    counter++;
    if (counter % 60 == 0) {
    // Compute current speed magnitude.
    double speed = sqrt(car->vx*car->vx + car->vy*car->vy);
    // Only log if the car is moving.
    if (speed > 0.01) {
        printf("[car %s] pos=(%.2f, %.2f) dest=(%.2f, %.2f)\n",
            car->license, car->x, car->y, car->dest_x, car->dest_y);
    }
  }
}

int main(int argc, char **argv) {
    // Usage:
    //   car_node.exe <license> <listen_port> <start_x> <start_y> <dest_x> <dest_y> [target_speed]
    const char *license = (argc >= 2) ? argv[1] : "CAR123";
    int listen_port      = (argc >= 3) ? atoi(argv[2]) : 9001;
    double start_x       = (argc >= 4) ? atof(argv[3]) : 0.0;
    double start_y       = (argc >= 5) ? atof(argv[4]) : 0.0;
    double dest_x        = (argc >= 6) ? atof(argv[5]) : 100.0;
    double dest_y        = (argc >= 7) ? atof(argv[6]) : 0.0;
    double target_speed  = (argc >= 8) ? atof(argv[7]) : 10.0;

    // Initialize libuv default event loop.
    g_loop = uv_default_loop();
    // Initialize car runtime state.
    CarRuntime car;
    memset(&car, 0, sizeof(car));
    // Copy license safely.
    strncpy(car.license, license, sizeof(car.license)-1);
    // Set initial position and destination.
    car.x = start_x; car.y = start_y;
    car.dest_x = dest_x; car.dest_y = dest_y;
    // Set motion parameters.
    car.target_speed = target_speed;
    car.max_accel = 3.0; // m/s^2 (tweakable)

    // (Optional) initialize your metadata struct too (not required for server right now)
    car_init(&car.meta, "1HGBH41JXMN109186", 2022, "Toyota", "Camry", start_x, start_y);

    // Start car listening server
    uv_tcp_t car_server;
    uv_tcp_init(g_loop, &car_server);
    // Attach CarRuntime to server handle for access in callbacks.
    car_server.data = &car;
    // Bind to 0.0.0.0:<listen_port>
    struct sockaddr_in addr;
    uv_ip4_addr("0.0.0.0", listen_port, &addr);
    uv_tcp_bind(&car_server, (const struct sockaddr*)&addr, 0);
    // Start listening for incoming connections.
    int r = uv_listen((uv_stream_t*)&car_server, 128, on_new_car_conn);
    if (r != 0) {
        fprintf(stderr, "car listen failed: %s\n", uv_strerror(r));
        return 1;
    }
    
    printf("[car_node] listening on http://0.0.0.0:%d\n", listen_port);

    // Register with central server (matches your current server.c)
    post_register_car("127.0.0.1", 8080, &car, listen_port);

    // Start simulation timer (16ms)
    uv_timer_t sim;
    uv_timer_init(g_loop, &sim);
    sim.data = &car;
    uv_timer_start(&sim, on_sim_timer, 16, 16);

    return uv_run(g_loop, UV_RUN_DEFAULT);
}

