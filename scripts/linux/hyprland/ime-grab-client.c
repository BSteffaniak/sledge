// Independent input-method-v2 keyboard-grab consumer, disposable guest only.
#define _POSIX_C_SOURCE 200809L
#include <wayland-client.h>
#include "input-method-client.h"
#include <poll.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
static struct wl_seat *seat;
static struct zwp_input_method_manager_v2 *manager;
static bool unavailable;
static unsigned keys, modifiers;
static void global(void *data, struct wl_registry *r, uint32_t name, const char *interface, uint32_t version) {
    (void)data; (void)version;
    if (!strcmp(interface, "wl_seat") && !seat) seat = wl_registry_bind(r, name, &wl_seat_interface, 1);
    if (!strcmp(interface, "zwp_input_method_manager_v2"))
        manager = wl_registry_bind(r, name, &zwp_input_method_manager_v2_interface, 1);
}
static void removed(void *data, struct wl_registry *r, uint32_t name) { (void)data; (void)r; (void)name; }
static void activate(void *data, struct zwp_input_method_v2 *ime) { (void)data; (void)ime; puts("IME_ACTIVATE"); }
static void deactivate(void *data, struct zwp_input_method_v2 *ime) { (void)data; (void)ime; puts("IME_DEACTIVATE"); }
static void surrounding(void *data, struct zwp_input_method_v2 *ime, const char *text, uint32_t cursor, uint32_t anchor) {
    (void)data; (void)ime; (void)text; (void)cursor; (void)anchor;
}
static void cause(void *data, struct zwp_input_method_v2 *ime, uint32_t value) { (void)data; (void)ime; (void)value; }
static void content(void *data, struct zwp_input_method_v2 *ime, uint32_t hint, uint32_t purpose) {
    (void)data; (void)ime; (void)hint; (void)purpose;
}
static void done(void *data, struct zwp_input_method_v2 *ime) { (void)data; (void)ime; puts("IME_DONE"); }
static void denied(void *data, struct zwp_input_method_v2 *ime) {
    (void)data; (void)ime; unavailable = true; puts("IME_UNAVAILABLE");
}
static const struct zwp_input_method_v2_listener ime_events = {activate, deactivate, surrounding, cause, content, done, denied};
static void keymap(void *data, struct zwp_input_method_keyboard_grab_v2 *grab, uint32_t format, int fd, uint32_t size) {
    (void)data; (void)grab; close(fd); printf("IME_KEYMAP format=%u size=%u\n", format, size);
}
static void key(void *data, struct zwp_input_method_keyboard_grab_v2 *grab, uint32_t serial, uint32_t time, uint32_t code, uint32_t state) {
    (void)data; (void)grab; (void)serial; ++keys;
    printf("IME_KEY code=%u state=%u time=%u\n", code, state, time);
}
static void mods(void *data, struct zwp_input_method_keyboard_grab_v2 *grab, uint32_t serial, uint32_t depressed, uint32_t latched, uint32_t locked, uint32_t group) {
    (void)data; (void)grab; (void)serial; ++modifiers;
    printf("IME_MODS depressed=%u latched=%u locked=%u group=%u\n", depressed, latched, locked, group);
}
static void repeat(void *data, struct zwp_input_method_keyboard_grab_v2 *grab, int32_t rate, int32_t delay) {
    (void)data; (void)grab; printf("IME_REPEAT rate=%d delay=%d\n", rate, delay);
}
static const struct zwp_input_method_keyboard_grab_v2_listener grab_events = {keymap, key, mods, repeat};
int main(void) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) return 1;
    struct wl_registry *registry = wl_display_get_registry(display);
    static const struct wl_registry_listener globals = {global, removed};
    wl_registry_add_listener(registry, &globals, NULL);
    if (wl_display_roundtrip(display) < 0 || !seat || !manager) return 2;
    struct zwp_input_method_v2 *ime = zwp_input_method_manager_v2_get_input_method(manager, seat);
    zwp_input_method_v2_add_listener(ime, &ime_events, NULL);
    struct zwp_input_method_keyboard_grab_v2 *grab = zwp_input_method_v2_grab_keyboard(ime);
    zwp_input_method_keyboard_grab_v2_add_listener(grab, &grab_events, NULL);
    if (wl_display_roundtrip(display) < 0 || unavailable) return 3;
    puts("IME_GRAB_READY");
    bool released = false;
    for (unsigned attempt = 0; attempt < 150; ++attempt) {
        if (wl_display_dispatch_pending(display) < 0 || wl_display_flush(display) < 0) return 4;
        struct pollfd fds[] = {{wl_display_get_fd(display), POLLIN, 0}, {STDIN_FILENO, POLLIN, 0}};
        if (poll(fds, 2, 100) < 0) return 5;
        if (fds[0].revents & POLLIN) if (wl_display_dispatch(display) < 0) return 6;
        if (fds[1].revents & POLLIN) {
            char command[32];
            ssize_t count = read(STDIN_FILENO, command, sizeof(command));
            if (count >= 8 && !memcmp(command, "release\n", 8)) { released = true; break; }
        }
    }
    zwp_input_method_keyboard_grab_v2_release(grab);
    zwp_input_method_v2_destroy(ime);
    if (wl_display_roundtrip(display) < 0) return 7;
    printf("IME_RELEASE keys=%u modifiers=%u\n", keys, modifiers);
    wl_display_disconnect(display);
    return released && keys && modifiers ? 0 : 8;
}
