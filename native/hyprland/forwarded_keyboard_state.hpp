#pragma once

#include "routing.hpp"
#include <xkbcommon/xkbcommon.h>

namespace sledge::hyprland {
// Consumer-route keymap, NEVER the physical/keybind/observational XKB state.
// Keymap construction allocates and must run outside production input callbacks.
// A keymap change invalidates pending decisions before this snapshot is replaced.
class ForwardedKeyboardState {
    std::array<bool,keyCount> held{};
    struct Prepared { xkb_keymap* map=nullptr; xkb_state* state=nullptr; };
    std::array<Prepared,deviceCount> prepared{};
    xkb_state* state = nullptr;
public:
    ForwardedKeyboardState() = default;
    ForwardedKeyboardState(const ForwardedKeyboardState&) = delete;
    ForwardedKeyboardState& operator=(const ForwardedKeyboardState&) = delete;
    ~ForwardedKeyboardState() {
        for (auto& entry : prepared) if (entry.state) xkb_state_unref(entry.state);
    }
    bool prepare(xkb_keymap* map) {
        if (!map) return false;
        for (const auto& entry : prepared) if (entry.map==map) return true;
        for (auto& entry : prepared) if (!entry.state) {
            auto* next=xkb_state_new(map);if (!next) return false;
            entry={map,next};return true;
        }
        return false;
    }
    static xkb_mod_mask_t remap(xkb_state* from,xkb_keymap* to,xkb_state_component component) noexcept {
        if (!from) return 0;
        auto* old=xkb_state_get_keymap(from);
        const auto mask=xkb_state_serialize_mods(from,component);
        xkb_mod_mask_t result=0;
        for (xkb_mod_index_t i=0;i<xkb_keymap_num_mods(old) && i<32;++i) {
            if (!(mask&(xkb_mod_mask_t{1}<<i))) continue;
            const auto index=xkb_keymap_mod_get_index(to,xkb_keymap_mod_get_name(old,i));
            if (index<32) result|=xkb_mod_mask_t{1}<<index;
        }
        return result;
    }
    bool select(xkb_keymap* map,uint32_t group=0) noexcept {
        for (const auto& entry : prepared) if (entry.map==map) {
            if (state!=entry.state) {
                const auto latched=remap(state,map,XKB_STATE_MODS_LATCHED);
                const auto locked=remap(state,map,XKB_STATE_MODS_LOCKED);
                state=entry.state;
                // Clear every key from this cached projection before rebuilding.
                for (uint32_t key=0;key<keyCount;++key) xkb_state_update_key(state,key+8,XKB_KEY_UP);
                xkb_state_update_mask(state,0,latched,locked,0,0,group);
                for (uint32_t key=0;key<keyCount;++key) if (held[key]) xkb_state_update_key(state,key+8,XKB_KEY_DOWN);
            }
            const auto depressed=xkb_state_serialize_mods(state,XKB_STATE_MODS_DEPRESSED);
            const auto latched=xkb_state_serialize_mods(state,XKB_STATE_MODS_LATCHED);
            const auto locked=xkb_state_serialize_mods(state,XKB_STATE_MODS_LOCKED);
            xkb_state_update_mask(state,depressed,latched,locked,0,0,group);
            return true;
        }
        return false;
    }
    bool setKeymap(xkb_keymap* map) { return prepare(map) && select(map); }
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
    void adoptReleasedNative(uint32_t latched,uint32_t locked,uint32_t group) noexcept {
        if (state) xkb_state_update_mask(state,0,latched,locked,0,0,group);
    }
    const std::array<bool,keyCount>& focusEnterKeys() const noexcept { return held; }
    uint32_t depressed() const noexcept { return state ? xkb_state_serialize_mods(state,XKB_STATE_MODS_DEPRESSED) : 0; }
    uint32_t latched() const noexcept { return state ? xkb_state_serialize_mods(state,XKB_STATE_MODS_LATCHED) : 0; }
    uint32_t locked() const noexcept { return state ? xkb_state_serialize_mods(state,XKB_STATE_MODS_LOCKED) : 0; }
    uint32_t group() const noexcept { return state ? xkb_state_serialize_layout(state,XKB_STATE_LAYOUT_EFFECTIVE) : 0; }
};
} // namespace sledge::hyprland
