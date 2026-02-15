// vehicle_emulator/car_node.c
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

static int parse_content_length(const char *req) {
    const char *v = find_header(req, "Content-Length");
    if (!v) return 0;
    return atoi(v);
}

static char* find_body(char *req) {
    char *body = strstr(req, "\r\n\r\n");
    if (!body) return NULL;
    return body + 4;
}

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

typedef struct {
    uv_tcp_t handle;
    CarRuntime *car;
} CarClient;

static void alloc_buf(uv_handle_t *h, size_t suggested, uv_buf_t *buf) {
    (void)h;
    buf->base = (char*)malloc(suggested);
    buf->len = suggested;
}

static void on_car_client_read(uv_stream_t *stream, ssize_t nread, const uv_buf_t *buf) {
    CarClient *cc = (CarClient*)stream;
    CarRuntime *car = cc->car;

    if (nread <= 0) {
        if (buf->base) free(buf->base);
        if (nread < 0) {
            uv_close((uv_handle_t*)stream, [](uv_handle_t *h){ free(h); });
        }
        return;
    }

    // Ensure request is null-terminated
    buf->base[nread] = '\0';
    char *req = buf->base;

    // Routes:
    // GET /status
    // POST /command (form: speed=..&dest_x=..&dest_y=..)
    if (starts_with(req, "GET /status")) {
        char body[512];
        snprintf(body, sizeof(body),
            "{"
              "\"license\":\"%s\","
              "\"x\":%.3f,\"y\":%.3f,"
              "\"vx\":%.3f,\"vy\":%.3f,"
              "\"dest_x\":%.3f,\"dest_y\":%.3f,"
              "\"target_speed\":%.3f"
            "}",
            car->license, car->x, car->y, car->vx, car->vy,
            car->dest_x, car->dest_y, car->target_speed
        );
        write_http_response(stream, "HTTP/1.1 200 OK", "application/json", body);
    }
    else if (starts_with(req, "POST /command")) {
        char *body = find_body(req);
        if (!body) {
            write_http_response(stream, "HTTP/1.1 400 Bad Request", "text/plain", "missing body");
        } else {
            // parse form-encoded values if present
            // accepted keys: speed, dest_x, dest_y
            // example: speed=8.5&dest_x=100&dest_y=0
            double new_speed = car->target_speed;
            double new_dx = car->dest_x;
            double new_dy = car->dest_y;

            // Very simple parsing: try sscanf in common order first, then fallback searches
            // sprint-safe: keep consistent on client side
            if (strstr(body, "speed=") != NULL) {
                sscanf(strstr(body, "speed="), "speed=%lf", &new_speed);
            }
            if (strstr(body, "dest_x=") != NULL) {
                sscanf(strstr(body, "dest_x="), "dest_x=%lf", &new_dx);
            }
            if (strstr(body, "dest_y=") != NULL) {
                sscanf(strstr(body, "dest_y="), "dest_y=%lf", &new_dy);
            }

            car->target_speed = new_speed;
            car->dest_x = new_dx;
            car->dest_y = new_dy;

            write_http_response(stream, "HTTP/1.1 200 OK", "text/plain", "ok");
        }
    }
    else {
        write_http_response(stream, "HTTP/1.1 404 Not Found", "text/plain", "not found");
    }

    free(buf->base);
}

static void on_new_car_conn(uv_stream_t *server, int status) {
    if (status < 0) return;

    uv_tcp_t *server_tcp = (uv_tcp_t*)server;
    CarRuntime *car = (CarRuntime*)server_tcp->data;

    CarClient *client = (CarClient*)malloc(sizeof(CarClient));
    if (!client) return;

    client->car = car;
    uv_tcp_init(g_loop, &client->handle);

    if (uv_accept(server, (uv_stream_t*)&client->handle) == 0) {
        uv_read_start((uv_stream_t*)&client->handle, alloc_buf, on_car_client_read);
    } else {
        uv_close((uv_handle_t*)&client->handle, [](uv_handle_t *h){ free(h); });
    }
}

// ---------------- Register with central server (simple libuv TCP client) ----------------

typedef struct {
    uv_tcp_t tcp;
    uv_connect_t conn;
    uv_write_t wr;
    char *req_mem;
} HttpClient;

static void on_http_client_closed(uv_handle_t *h) {
    HttpClient *hc = (HttpClient*)h->data;
    if (hc) {
        free(hc->req_mem);
        free(hc);
    }
}

static void on_http_write_done(uv_write_t *wr, int status) {
    (void)status;
    // close after write; we don't need to read response for sprint 1
    uv_close((uv_handle_t*)wr->handle, on_http_client_closed);
}

static void on_http_connected(uv_connect_t *req, int status) {
    if (status < 0) {
        fprintf(stderr, "[car_node] failed to connect to central server\n");
        HttpClient *hc = (HttpClient*)req->handle->data;
        uv_close((uv_handle_t*)&hc->tcp, on_http_client_closed);
        return;
    }

    HttpClient *hc = (HttpClient*)req->handle->data;
    uv_buf_t b = uv_buf_init(hc->req_mem, (unsigned int)strlen(hc->req_mem));
    uv_write(&hc->wr, req->handle, &b, 1, on_http_write_done);
}

static void post_register_car(const char *host_ip, int port, CarRuntime *car) {
    // Build form body to match your server.c
    char body[256];
    snprintf(body, sizeof(body),
        "license=%s&start_x=%.3f&start_y=%.3f&dest_x=%.3f&dest_y=%.3f",
        car->license, car->x, car->y, car->dest_x, car->dest_y
    );

    char req[512];
    int body_len = (int)strlen(body);
    snprintf(req, sizeof(req),
        "POST /register-car HTTP/1.1\r\n"
        "Host: %s:%d\r\n"
        "Content-Type: application/x-www-form-urlencoded\r\n"
        "Content-Length: %d\r\n"
        "Connection: close\r\n"
        "\r\n"
        "%s",
        host_ip, port, body_len, body
    );

    HttpClient *hc = (HttpClient*)calloc(1, sizeof(HttpClient));
    if (!hc) return;

    hc->req_mem = _strdup(req);
    if (!hc->req_mem) { free(hc); return; }

    uv_tcp_init(g_loop, &hc->tcp);
    hc->tcp.data = hc;

    struct sockaddr_in dest;
    uv_ip4_addr(host_ip, port, &dest);

    uv_tcp_connect(&hc->conn, &hc->tcp, (const struct sockaddr*)&dest, on_http_connected);
}

// ---------------- Simulation timer ----------------

static void on_sim_timer(uv_timer_t *t) {
    CarRuntime *car = (CarRuntime*)t->data;
    const double dt = 0.016; // 16ms

    sim_tick(car, dt);

    // optional console log every ~1s
    static int counter = 0;
    counter++;
    if (counter % 60 == 0) {
        printf("[car %s] pos=(%.2f, %.2f) vel=(%.2f, %.2f) dest=(%.2f, %.2f) target_speed=%.2f\n",
            car->license, car->x, car->y, car->vx, car->vy, car->dest_x, car->dest_y, car->target_speed);
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

    g_loop = uv_default_loop();

    CarRuntime car;
    memset(&car, 0, sizeof(car));
    strncpy(car.license, license, sizeof(car.license)-1);
    car.x = start_x; car.y = start_y;
    car.dest_x = dest_x; car.dest_y = dest_y;
    car.target_speed = target_speed;
    car.max_accel = 3.0; // m/s^2 (tweakable)

    // (Optional) initialize your metadata struct too (not required for server right now)
    car_init(&car.meta, "1HGBH41JXMN109186", 2022, "Toyota", "Camry", start_x, start_y);

    // Start car listening server
    uv_tcp_t car_server;
    uv_tcp_init(g_loop, &car_server);
    car_server.data = &car;

    struct sockaddr_in addr;
    uv_ip4_addr("0.0.0.0", listen_port, &addr);
    uv_tcp_bind(&car_server, (const struct sockaddr*)&addr, 0);

    int r = uv_listen((uv_stream_t*)&car_server, 128, on_new_car_conn);
    if (r != 0) {
        fprintf(stderr, "car listen failed: %s\n", uv_strerror(r));
        return 1;
    }

    printf("[car_node] listening on http://0.0.0.0:%d\n", listen_port);

    // Register with central server (matches your current server.c)
    post_register_car("127.0.0.1", 8080, &car);

    // Start simulation timer (16ms)
    uv_timer_t sim;
    uv_timer_init(g_loop, &sim);
    sim.data = &car;
    uv_timer_start(&sim, on_sim_timer, 16, 16);

    return uv_run(g_loop, UV_RUN_DEFAULT);
}
