// Independent complete ext-session-lock consumer; isolated guest use only.
#define _POSIX_C_SOURCE 200809L
#include <wayland-client.h>
#include "session-lock-client.h"
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

#define MAX_OUTPUTS 16
struct output {
    struct wl_output *output;
    struct wl_surface *surface;
    struct ext_session_lock_surface_v1 *lock_surface;
    struct wl_buffer *buffer;
    void *pixels;
    size_t size;
    unsigned index;
    bool configured;
};
static struct wl_compositor *compositor;
static struct wl_shm *shm;
static struct wl_seat *seat;
static struct wl_keyboard *keyboard;
static struct ext_session_lock_manager_v1 *manager;
static struct output outputs[MAX_OUTPUTS];
static unsigned output_count;
static bool acknowledged, finished, failed;

static void configure(void *data, struct ext_session_lock_surface_v1 *surface,
                      uint32_t serial, uint32_t width, uint32_t height) {
    struct output *o = data;
    if (!width || !height || width > 8192 || height > 8192) { failed = true; return; }
    size_t size = (size_t)width * height * 4;
    char path[] = "/tmp/stock-lock-shm-XXXXXX";
    int fd = mkstemp(path);
    if (fd < 0) { failed = true; return; }
    unlink(path);
    if (ftruncate(fd, (off_t)size) < 0) { close(fd); failed = true; return; }
    void *pixels = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (pixels == MAP_FAILED) { close(fd); failed = true; return; }
    struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, (int32_t)size);
    struct wl_buffer *buffer = wl_shm_pool_create_buffer(pool, 0, (int32_t)width,
        (int32_t)height, (int32_t)width * 4, WL_SHM_FORMAT_XRGB8888);
    wl_shm_pool_destroy(pool);
    close(fd);
    // Retain the current buffer until disconnect. Resize is not part of this fixture.
    if (o->buffer) wl_buffer_destroy(o->buffer);
    if (o->pixels) munmap(o->pixels, o->size);
    o->pixels = pixels; o->size = size; o->buffer = buffer;
    for (size_t i = 0; i < size / 4; ++i) ((uint32_t *)pixels)[i] = 0x00203040;
    ext_session_lock_surface_v1_ack_configure(surface, serial);
    wl_surface_attach(o->surface, buffer, 0, 0);
    wl_surface_damage(o->surface, 0, 0, (int32_t)width, (int32_t)height);
    wl_surface_commit(o->surface);
    o->configured = true;
    printf("LOCK_SURFACE output=%u width=%u height=%u ack=%u committed=1\n",
        o->index, width, height, serial);
}
static const struct ext_session_lock_surface_v1_listener surface_events = {configure};
static void keymap(void *data, struct wl_keyboard *k, uint32_t format, int fd, uint32_t size) {
    (void)data; (void)k; close(fd);
    printf("LOCK_KEYMAP format=%u size=%u\n", format, size);
}
static void enter(void *data, struct wl_keyboard *k, uint32_t serial,
                  struct wl_surface *surface, struct wl_array *keys) {
    (void)data; (void)k; (void)serial; (void)surface;
    printf("LOCK_ENTER held=");
    uint32_t *key;
    wl_array_for_each(key, keys) printf("%u,", *key);
    puts("");
}
static void leave(void *data, struct wl_keyboard *k, uint32_t serial, struct wl_surface *s) {
    (void)data; (void)k; (void)serial; (void)s; puts("LOCK_LEAVE");
}
static void key(void *data, struct wl_keyboard *k, uint32_t serial, uint32_t time,
                uint32_t code, uint32_t state) {
    (void)data; (void)k; (void)serial;
    printf("LOCK_KEY code=%u state=%u time=%u ack=%d\n", code, state, time, acknowledged);
}
static void mods(void *data, struct wl_keyboard *k, uint32_t serial, uint32_t depressed,
                 uint32_t latched, uint32_t locked, uint32_t group) {
    (void)data; (void)k; (void)serial;
    printf("LOCK_MODS depressed=%u latched=%u locked=%u group=%u\n", depressed, latched, locked, group);
}
static void repeat(void *data, struct wl_keyboard *k, int32_t rate, int32_t delay) {
    (void)data; (void)k; printf("LOCK_REPEAT rate=%d delay=%d\n", rate, delay);
}
static const struct wl_keyboard_listener keyboard_events = {keymap, enter, leave, key, mods, repeat};
static void capabilities(void *data, struct wl_seat *s, uint32_t caps) {
    (void)data;
    if ((caps & WL_SEAT_CAPABILITY_KEYBOARD) && !keyboard) {
        keyboard = wl_seat_get_keyboard(s);
        wl_keyboard_add_listener(keyboard, &keyboard_events, NULL);
    }
}
static void seat_name(void *data, struct wl_seat *s, const char *name) {
    (void)data; (void)s; printf("LOCK_SEAT name=%s\n", name);
}
static const struct wl_seat_listener seat_events = {capabilities, seat_name};
static void global(void *data, struct wl_registry *registry, uint32_t name,
                   const char *interface, uint32_t version) {
    (void)data;
    if (!strcmp(interface, "ext_session_lock_manager_v1"))
        manager = wl_registry_bind(registry, name, &ext_session_lock_manager_v1_interface, 1);
    else if (!strcmp(interface, "wl_compositor"))
        compositor = wl_registry_bind(registry, name, &wl_compositor_interface, 4);
    else if (!strcmp(interface, "wl_shm")) shm = wl_registry_bind(registry, name, &wl_shm_interface, 1);
    else if (!strcmp(interface, "wl_seat") && !seat) {
        seat = wl_registry_bind(registry, name, &wl_seat_interface, version < 5 ? version : 5);
        wl_seat_add_listener(seat, &seat_events, NULL);
    } else if (!strcmp(interface, "wl_output")) {
        if (output_count == MAX_OUTPUTS) { failed = true; return; }
        struct output *o = &outputs[output_count];
        o->index = output_count++;
        o->output = wl_registry_bind(registry, name, &wl_output_interface, 1);
    }
}
static void removed(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data; (void)registry; (void)name;
    // Hotplug is outside this single fixed-output fixture.
    failed = true;
    puts("LOCK_GLOBAL_REMOVED_UNQUALIFIED");
}
static void locked(void *data, struct ext_session_lock_v1 *lock) {
    (void)data; (void)lock; acknowledged = true; puts("LOCK_ACK");
}
static void done(void *data, struct ext_session_lock_v1 *lock) {
    (void)data; (void)lock; finished = true; puts("LOCK_FINISHED");
}
static int64_t now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (int64_t)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}
int main(void) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) return 1;
    struct wl_registry *registry = wl_display_get_registry(display);
    static const struct wl_registry_listener globals = {global, removed};
    wl_registry_add_listener(registry, &globals, NULL);
    if (wl_display_roundtrip(display) < 0 || !manager || !compositor || !shm || !output_count || failed) return 2;
    struct ext_session_lock_v1 *lock = ext_session_lock_manager_v1_lock(manager);
    static const struct ext_session_lock_v1_listener events = {locked, done};
    ext_session_lock_v1_add_listener(lock, &events, NULL);
    for (unsigned i = 0; i < output_count; ++i) {
        struct output *o = &outputs[i];
        o->surface = wl_compositor_create_surface(compositor);
        o->lock_surface = ext_session_lock_v1_get_lock_surface(lock, o->surface, o->output);
        ext_session_lock_surface_v1_add_listener(o->lock_surface, &surface_events, o);
    }
    printf("LOCK_REQUEST outputs=%u\n", output_count);
    int64_t deadline = now_ms() + 15000;
    bool unlock = false;
    while (!failed && !finished && now_ms() < deadline) {
        if (wl_display_dispatch_pending(display) < 0) break;
        if (wl_display_flush(display) < 0 && errno != EAGAIN) break;
        struct pollfd fds[] = {{wl_display_get_fd(display), POLLIN, 0}, {STDIN_FILENO, POLLIN, 0}};
        int result = poll(fds, 2, 100);
        if (result < 0 && errno == EINTR) continue;
        if (result < 0) break;
        if (fds[0].revents & POLLIN) {
            if (wl_display_dispatch(display) < 0) break;
        }
        if (fds[0].revents & (POLLERR | POLLHUP)) break;
        if (fds[1].revents & POLLIN) {
            char command[32];
            ssize_t count = read(STDIN_FILENO, command, sizeof(command));
            if (count >= 7 && !memcmp(command, "unlock\n", 7) && acknowledged) { unlock = true; break; }
            if (count >= 6 && !memcmp(command, "crash\n", 6)) {
                puts("LOCK_ABRUPT_DISCONNECT");
                _exit(42); // Deliberately no unlock or protocol cleanup.
            }
        }
    }
    if (!unlock) { puts("LOCK_NO_UNLOCK_FAIL"); return 3; }
    for (unsigned i = 0; i < output_count; ++i) {
        if (!outputs[i].configured) return 4;
        ext_session_lock_surface_v1_destroy(outputs[i].lock_surface);
        wl_surface_destroy(outputs[i].surface);
    }
    ext_session_lock_v1_unlock_and_destroy(lock);
    if (wl_display_roundtrip(display) < 0) return 5;
    puts("LOCK_UNLOCK_ROUNDTRIP");
    wl_display_disconnect(display);
    return 0;
}
