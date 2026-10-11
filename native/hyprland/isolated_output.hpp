#pragma once
#include "routing.hpp"

namespace sledge::hyprland {
// Required compositor adapter interface: reserve/preflight checks focus, lock
// epoch, keymap and complete route availability BEFORE any observable output.
// Each method executes on the compositor thread, without yielding/re-entry.
struct KeyRoute {
    virtual ~KeyRoute() = default;
    virtual bool preflight(uint32_t key, uint8_t modifiers) = 0;
    virtual bool modifiers(uint8_t sides) = 0;
    virtual bool key(uint32_t code, bool down) = 0;
};
class IsolatedOutput : public Output {
    KeyRoute& route;
public:
    explicit IsolatedOutput(KeyRoute& route_) : route(route_) {}
    Delivery replace(uint32_t key, uint8_t requested, uint8_t restore) override {
        if (!route.preflight(key, requested)) return Delivery::RejectedNoOutput;
        // Even failure of the first write is conservatively partial: the route
        // cannot prove no client observed it. Never send the original afterward.
        if (!route.modifiers(requested)) {
            (void)route.modifiers(restore);
            return Delivery::PartialFatal;
        }
        const bool down = route.key(key, true);
        const bool up = route.key(key, false);
        const bool restored = route.modifiers(restore);
        return down && up && restored ? Delivery::Accepted : Delivery::PartialFatal;
    }
    bool release(uint32_t key) override { return route.key(key, false); }
    bool reconcileModifiers(uint8_t sides) override { return route.modifiers(sides); }
};
} // namespace sledge::hyprland
