#pragma once
#include "stock_research_route.hpp"
#include "decision_mailbox.hpp"
#include "replacement_emission.hpp"

namespace sledge::hyprland {
// Narrow stock seat reservation, compositor thread only. Binding is observed
// at actual sendEnter; exact owner/focus must still match at commit. Strong refs
// reserve resource lifetime for the whole pair. No IME route is admitted.
class StockReplacement {
    struct Binding { WP<CWLKeyboardResource> keyboard; WP<CWLSurfaceResource> surface; };
    std::array<Binding,16> bindings{};
    xkb_state* modifiers = nullptr;
public:
    uint32_t observedCapabilities = 0;
    bool capabilitiesKnown = false;
    bool rejectBefore = false;
    uint64_t deadline = 0;
    unsigned failAfter = 0; // isolated deterministic partial-output fixture
    ~StockReplacement() { if (modifiers) xkb_state_unref(modifiers); }
    bool setKeymap(xkb_keymap* map) {
        auto* next=map ? xkb_state_new(map) : nullptr;
        if (!next) return false;
        if (modifiers) xkb_state_unref(modifiers);
        modifiers=next; return true;
    }
    void entered(CWLKeyboardResource* resource, SP<CWLSurfaceResource> surface) {
        auto owner=resource->m_owner.lock();
        if (!owner) return;
        for (const auto& weak : owner->m_keyboards) {
            auto keyboard=weak.lock();
            if (keyboard.get()!=resource) continue;
            for (auto& binding : bindings) {
                if (!binding.keyboard.lock() || binding.keyboard.lock()==keyboard) {
                    binding={keyboard,surface}; return;
                }
            }
        }
    }
    Delivery replace(uint32_t key, uint8_t requested, ResearchRoute& route) {
        auto focus=g_pSeatManager->m_state.keyboardFocus.lock();
        auto owner=g_pSeatManager->m_state.keyboardFocusResource.lock();
        if (rejectBefore || !modifiers || !focus || !owner || route.phase.routeLocked() || route.revoking ||
            route.output.ime.lock() || key>=keyCount || route.router.targetHeld(key) ||
            !capabilitiesKnown || !(observedCapabilities&HID_INPUT_CAPABILITY_KEYBOARD) ||
            !deadline || monotonicNs()+500'000>=deadline) return Delivery::RejectedNoOutput;
        std::array<SP<CWLKeyboardResource>,16> reserved{}; unsigned count=0;
        for (const auto& weak : owner->m_keyboards) {
            auto keyboard=weak.lock();
            if (!keyboard) continue;
            if (!keyboard->good() || count==reserved.size()) return Delivery::RejectedNoOutput;
            bool bound=false;
            for (const auto& binding : bindings)
                if (binding.keyboard.lock()==keyboard && binding.surface.lock()==focus && keyboard->m_owner.lock()==owner) bound=true;
            if (!bound) return Delivery::RejectedNoOutput;
            reserved[count++]=keyboard;
        }
        if (!count) return Delivery::RejectedNoOutput;
        constexpr std::array<uint32_t,8> sides{42,54,29,97,56,100,125,126};
        // Reset the synthetic-only modifier state so Caps/latched history from
        // earlier fixture requests cannot contaminate a later replacement.
        xkb_state_update_mask(modifiers,0,0,0,0,0,route.forwarded.group());
        for (unsigned i=0; i<8; ++i) xkb_state_update_key(modifiers,sides[i]+8,(requested&(1U<<i)) ? XKB_KEY_DOWN : XKB_KEY_UP);
        const auto mask=xkb_state_serialize_mods(modifiers,XKB_STATE_MODS_DEPRESSED);
        auto sendMods=[&](uint32_t depressed,uint32_t latched,uint32_t locked,uint32_t group) {
            for (unsigned i=0; i<count; ++i)
                reinterpret_cast<void (*)(CWLKeyboardResource*,uint32_t,uint32_t,uint32_t,uint32_t)>(route.hooks.original(1))
                    (reserved[i].get(),depressed,latched,locked,group);
        };
        // Direct resource methods, no InputManager/keybind/matcher reentry.
        const bool complete=emitReplacement(
            [&] { sendMods(mask,0,0,route.forwarded.group()); },
            [&](bool down) {
                for (unsigned i=0;i<count;++i) reserved[i]->sendKey(route.output.time,key,
                    down ? WL_KEYBOARD_KEY_STATE_PRESSED : WL_KEYBOARD_KEY_STATE_RELEASED);
            },
            [&] { sendMods(route.forwarded.depressed(),route.forwarded.latched(),route.forwarded.locked(),route.forwarded.group()); },failAfter);
        return !complete || monotonicNs()>=deadline ? Delivery::PartialFatal : Delivery::Accepted;
    }
};
} // namespace sledge::hyprland
