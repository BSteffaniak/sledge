#pragma once
#include <array>
#include <cstdint>
#include <xkbcommon/xkbcommon.h>

// Observed stock admission, separate from physical/keybind XKB. NOT a
// consumer-delivery ledger: MUST NOT control focus enter, modifiers, or routing.
// Compositor-thread only. No transport, capture, modifier-policy override.
class DeliveredKeyboardState {
    std::array<bool, 768> held{};
    xkb_state* state = nullptr;
public:
    DeliveredKeyboardState() = default;
    DeliveredKeyboardState(const DeliveredKeyboardState&) = delete;
    DeliveredKeyboardState& operator=(const DeliveredKeyboardState&) = delete;
    ~DeliveredKeyboardState() { if (state) xkb_state_unref(state); }
    bool setKeymap(xkb_keymap* keymap) {
        auto* next = keymap ? xkb_state_new(keymap) : nullptr;
        if (!next) return false; // old snapshot remains valid on allocation failure
        for (uint32_t key = 0; key < held.size(); ++key)
            if (held[key]) xkb_state_update_key(next, key + 8, XKB_KEY_DOWN);
        if (state) xkb_state_unref(state);
        state = next;
        return true;
    }
    void key(uint32_t code, bool down) {
        if (code >= held.size() || held[code] == down) return;
        held[code] = down;
        if (state) xkb_state_update_key(state, code + 8, down ? XKB_KEY_DOWN : XKB_KEY_UP);
    }
    bool contains(uint32_t code) const { return code < held.size() && held[code]; }
    uint32_t depressed() const { return state ? xkb_state_serialize_mods(state, XKB_STATE_MODS_DEPRESSED) : 0; }
    void clear() {
        for (uint32_t code = 0; code < held.size(); ++code) key(code, false);
    }
};
