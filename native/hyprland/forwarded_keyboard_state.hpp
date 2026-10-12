#pragma once

#include "routing.hpp"
#include <xkbcommon/xkbcommon.h>

namespace sledge::hyprland {
// Consumer-route keymap, NEVER the physical/keybind/observational XKB state.
// Keymap construction allocates and must run outside production input callbacks.
// A keymap change invalidates pending decisions before this snapshot is replaced.
class ForwardedKeyboardState {
    std::array<bool,keyCount> held{};
    xkb_state* state = nullptr;
public:
    ForwardedKeyboardState() = default;
    ForwardedKeyboardState(const ForwardedKeyboardState&) = delete;
    ForwardedKeyboardState& operator=(const ForwardedKeyboardState&) = delete;
    ~ForwardedKeyboardState() { if (state) xkb_state_unref(state); }
    bool setKeymap(xkb_keymap* map) {
        auto* next = map ? xkb_state_new(map) : nullptr;
        if (!next) return false;
        for (uint32_t key = 0; key < keyCount; ++key)
            if (held[key]) xkb_state_update_key(next,key + 8,XKB_KEY_DOWN);
        if (state) xkb_state_unref(state);
        state = next;
        return true;
    }
    // Call AFTER originalOutcome, not after a speculative Pass proposal. The
    // Router union handles merged devices and excludes consumed/locker holds.
    bool synchronize(const Router& router, bool lockerRoute = false) noexcept {
        if (!state) return false;
        const auto next = lockerRoute ? router.lockerEnterKeys() : router.focusEnterKeys();
        for (uint32_t key = 0; key < keyCount; ++key) {
            if (held[key] == next[key]) continue;
            xkb_state_update_key(state,key + 8,next[key] ? XKB_KEY_DOWN : XKB_KEY_UP);
        }
        held = next;
        return true;
    }
    const std::array<bool,keyCount>& focusEnterKeys() const noexcept { return held; }
    uint32_t depressed() const noexcept { return state ? xkb_state_serialize_mods(state,XKB_STATE_MODS_DEPRESSED) : 0; }
    uint32_t latched() const noexcept { return state ? xkb_state_serialize_mods(state,XKB_STATE_MODS_LATCHED) : 0; }
    uint32_t locked() const noexcept { return state ? xkb_state_serialize_mods(state,XKB_STATE_MODS_LOCKED) : 0; }
    uint32_t group() const noexcept { return state ? xkb_state_serialize_layout(state,XKB_STATE_LAYOUT_EFFECTIVE) : 0; }
};
} // namespace sledge::hyprland
