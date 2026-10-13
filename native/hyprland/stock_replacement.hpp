#pragma once
#include "stock_research_route.hpp"
#include "decision_mailbox.hpp"
#include "output_recovery.hpp"
#include "replacement_emission.hpp"

namespace sledge::hyprland {
// Narrow stock seat reservation, compositor thread only. Binding is observed
// at actual sendEnter; exact owner/focus must still match at commit. Strong refs
// reserve resource lifetime for the whole pair. No IME route is admitted.
class StockReplacement {
    struct Binding { WP<CWLKeyboardResource> keyboard; WP<CWLSurfaceResource> surface; };
    std::array<Binding,16> bindings{};
    struct Prepared { xkb_keymap* map=nullptr; xkb_state* state=nullptr; };
    std::array<Prepared,deviceCount> prepared{};
    xkb_state* modifiers = nullptr;
public:
    enum class Reject : unsigned { Fixture, Map, Focus, Locked, Ime, Target, Capabilities, Deadline, Resource, Binding, Empty, Count };
    std::array<uint64_t,static_cast<unsigned>(Reject::Count)> rejected{};
    uint64_t attempted=0,accepted=0,partial=0;
    Delivery reject(Reject reason) noexcept { ++rejected[static_cast<unsigned>(reason)];return Delivery::RejectedNoOutput; }
    std::string preflightStatus() const {
        constexpr std::array<const char*,static_cast<unsigned>(Reject::Count)> names{
            "fixture","map","focus","locked","ime","target","capabilities","deadline","resource","binding","empty"};
        std::string result="{\"attempted\":"+std::to_string(attempted)+",\"accepted\":"+std::to_string(accepted)+",\"partial\":"+std::to_string(partial);
        for (unsigned i=0;i<names.size();++i) result+=",\""+std::string(names[i])+"\":"+std::to_string(rejected[i]);
        return result+"}";
    }
    OutputRecovery recovery;
    uint32_t observedCapabilities = 0;
    bool capabilitiesKnown = false;
    bool rejectBefore = false;
    uint64_t deadline = 0;
    unsigned failAfter = 0; // isolated deterministic partial-output fixture
    ~StockReplacement() { for (auto& entry : prepared) if (entry.state) xkb_state_unref(entry.state); }
    bool prepare(xkb_keymap* map) {
        if (!map) return false;
        for (const auto& entry : prepared) if (entry.map==map) return true;
        for (auto& entry : prepared) if (!entry.state) {
            auto* next=xkb_state_new(map);if (!next) return false;
            entry={map,next};return true;
        }
        return false;
    }
    bool preparedFor(xkb_keymap* map) const noexcept {
        for (const auto& entry : prepared) if (entry.map==map) return true;
        return false;
    }
    bool select(xkb_keymap* map) noexcept {
        for (const auto& entry : prepared) if (entry.map==map) { modifiers=entry.state;return true; }
        return false;
    }
    bool setKeymap(xkb_keymap* map) { return prepare(map) && select(map); }
    void entered(CWLKeyboardResource* resource, SP<CWLSurfaceResource> surface) {
        auto owner=resource->m_owner.lock();
        if (!owner) return;
        for (const auto& weak : owner->m_keyboards) {
            auto keyboard=weak.lock();
            if (keyboard.get()!=resource) continue;
            for (auto& binding : bindings) {
                if (!binding.keyboard.lock() || binding.keyboard.lock()==keyboard) {
                    binding={keyboard,surface}; recovery.entered(); return;
                }
            }
        }
    }
    bool recoveryBindingQualified(ResearchRoute& route) const {
        const auto focus=g_pSeatManager->m_state.keyboardFocus.lock();
        const auto owner=g_pSeatManager->m_state.keyboardFocusResource.lock();
        if (!focus || !owner || route.phase.routeLocked() || route.output.ime.lock() ||
            !capabilitiesKnown || !(observedCapabilities&HID_INPUT_CAPABILITY_KEYBOARD)) return false;
        unsigned count=0;
        for (const auto& weak : owner->m_keyboards) {
            const auto keyboard=weak.lock();
            if (!keyboard) continue;
            bool bound=false;
            for (const auto& binding : bindings)
                if (binding.keyboard.lock()==keyboard && binding.surface.lock()==focus && keyboard->m_owner.lock()==owner) bound=true;
            if (!bound || !keyboard->good()) return false;
            ++count;
        }
        return count!=0;
    }
    Delivery replace(uint32_t key, uint8_t requested, ResearchRoute& route) {
        auto focus=g_pSeatManager->m_state.keyboardFocus.lock();
        auto owner=g_pSeatManager->m_state.keyboardFocusResource.lock();
        ++attempted;
        if (rejectBefore) return reject(Reject::Fixture);
        if (!modifiers) return reject(Reject::Map);
        if (!focus || !owner) return reject(Reject::Focus);
        if (route.phase.routeLocked() || route.revoking) return reject(Reject::Locked);
        if (route.output.ime.lock()) return reject(Reject::Ime);
        if (key>=keyCount || route.router.targetHeld(key)) return reject(Reject::Target);
        if (!capabilitiesKnown || !(observedCapabilities&HID_INPUT_CAPABILITY_KEYBOARD)) return reject(Reject::Capabilities);
        if (!deadline || monotonicNs()+500'000>=deadline) return reject(Reject::Deadline);
        std::array<SP<CWLKeyboardResource>,16> reserved{}; unsigned count=0;
        for (const auto& weak : owner->m_keyboards) {
            auto keyboard=weak.lock();
            if (!keyboard) continue;
            if (!keyboard->good() || count==reserved.size()) return reject(Reject::Resource);
            bool bound=false;
            for (const auto& binding : bindings)
                if (binding.keyboard.lock()==keyboard && binding.surface.lock()==focus && keyboard->m_owner.lock()==owner) bound=true;
            if (!bound) return reject(Reject::Binding);
            reserved[count++]=keyboard;
        }
        if (!count) return reject(Reject::Empty);
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
        if (!complete || monotonicNs()>=deadline) {
            ++partial;
            recovery.begin();
            for (unsigned i=0;i<count;++i) recovery.include(reserved[i].get());
            return Delivery::PartialFatal;
        }
        ++accepted;
        return Delivery::Accepted;
    }
};
} // namespace sledge::hyprland
