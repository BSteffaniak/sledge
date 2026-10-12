#pragma once
// Compiled only into explicit isolated research target; no production admission.
#include "stock_hooks.hpp"
#include "physical_devices.hpp"
#include "forwarded_keyboard_state.hpp"
#include "keymap_equivalence.hpp"
#include "lock_phase.hpp"
#include "initial_reconciliation.hpp"
#include <hyprland/src/protocols/core/Seat.hpp>
#include <hyprland/src/managers/KeybindManager.hpp>
#include <hyprland/src/managers/SeatManager.hpp>
#include <hyprland/src/managers/input/InputManager.hpp>
#include <hyprland/src/protocols/InputMethodV2.hpp>
#include <hyprland/src/protocols/SessionLock.hpp>
#include <aquamarine/input/Input.hpp>
#include <libinput.h>

namespace sledge::hyprland {
// Exact extra signatures needed because setKeyboardFocus emits directly through
// CWLKeyboardResource, and stock keybind admission is independent of seat output.
class ResearchHooks {
    HookTransaction<CFunctionHook,7> transaction;
    std::array<CFunctionHook*,7> hooks{};
    std::array<HookBytes,7> bytes{};
public:
    ~ResearchHooks() { reset(); }
    void install(HANDLE owner, const std::array<void*,7>& destinations) {
        const std::array<std::string,7> names{"sendEnter","sendMods","onKeyEvent","updateCapabilities","newKeyboard","destroyKeyboard","setKeyboardLayout"};
        const std::array<std::string,7> signatures{
            "CWLKeyboardResource::sendEnter(Hyprutils::Memory::CSharedPointer<CWLSurfaceResource>, wl_array*)",
            "CWLKeyboardResource::sendMods(unsigned int, unsigned int, unsigned int, unsigned int)",
            "CKeybindManager::onKeyEvent(std::any, Hyprutils::Memory::CSharedPointer<IKeyboard>)",
            "CWLSeatProtocol::updateCapabilities(unsigned int)",
            "CInputManager::newKeyboard(Hyprutils::Memory::CSharedPointer<IKeyboard>)",
            "CInputManager::destroyKeyboard(Hyprutils::Memory::CSharedPointer<IKeyboard>)",
            "CInputManager::setKeyboardLayout()"};
        const std::array<std::string,3> lifecycleSymbols{
            "_ZN13CInputManager11newKeyboardEN9Hyprutils6Memory14CSharedPointerI9IKeyboardEE",
            "_ZN13CInputManager15destroyKeyboardEN9Hyprutils6Memory14CSharedPointerI9IKeyboardEE",
            "_ZN13CInputManager17setKeyboardLayoutEv"};
        std::array<void*,7> sources{};
        for (unsigned i=0; i<7; ++i) {
            for (const auto& match : HyprlandAPI::findFunctionsByName(owner,names[i])) {
                // Stock pairs two nm outputs by line index. Match verified ELF
                // names for overloaded lifecycle APIs, not that demangle pairing.
                if (i>=4 ? match.signature!=lifecycleSymbols[i-4] : match.demangled!=signatures[i]) continue;
                if (sources[i] || !match.address) throw std::runtime_error("ambiguous research signature");
                sources[i]=match.address;
            }
            if (!sources[i] || !destinations[i]) throw std::runtime_error("missing research signature: "+signatures[i]);
            bytes[i].snapshot(sources[i]);
        }
        try {
            for (unsigned i=0; i<7; ++i) {
                hooks[i]=HyprlandAPI::createFunctionHook(owner,sources[i],destinations[i]);
                if (!transaction.install(hooks[i]) || bytes[i].restored()) throw std::runtime_error("research hook installation failed");
            }
        } catch (...) { reset(); throw; }
    }
    void reset() noexcept {
        transaction.rollback();
        for (auto& snapshot : bytes) { snapshot.requireRestored(); snapshot.clear(); }
        hooks.fill(nullptr);
    }
    void* original(unsigned i) const noexcept { return hooks[i] ? hooks[i]->m_original : nullptr; }
};
// Real synchronous old-route writes are attempted; void sends are conservatively
// UNCONFIRMED, never a delivery acknowledgement or replacement admission.
class ResearchOutput : public Output {
    StockHooks& hooks;
public:
    WP<CInputMethodV2> ime;
    WP<CWLSeatResource> seat;
    uint32_t time = 0;
    explicit ResearchOutput(StockHooks& stock) : hooks(stock) {}
    Delivery replace(uint32_t, uint8_t, uint8_t) override { return Delivery::RejectedNoOutput; }
    bool release(uint32_t key) override {
        if (auto grab=ime.lock())
            grab->sendKey(time,key,WL_KEYBOARD_KEY_STATE_RELEASED);
        else if (auto owner=seat.lock()) {
            for (const auto& weak : owner->m_keyboards)
                if (auto keyboard=weak.lock()) keyboard->sendKey(time,key,WL_KEYBOARD_KEY_STATE_RELEASED);
        }
        return false; // actual wire observation required; SDK is void
    }
    bool reconcileModifiers(uint8_t) override {
        if (auto grab=ime.lock())
            grab->sendMods(0,0,0,0);
        else if (auto owner=seat.lock()) {
            for (const auto& weak : owner->m_keyboards)
                if (auto keyboard=weak.lock()) keyboard->sendMods(0,0,0,0);
        }
        return false;
    }
};
class ResearchRoute {
    struct Device {
        WP<IKeyboard> keyboard;
        xkb_keymap* map = nullptr;
        uint64_t generation = 0;
    };
    std::array<Device,deviceCount> registered{};
    PhysicalDevices devices;
    xkb_keymap* map = nullptr;
public:
    Router router;
    ForwardedKeyboardState forwarded;
    ResearchOutput output;
    ResearchHooks hooks;
    std::optional<Event> current;
    const IKeyboard* currentKeyboard = nullptr;
    RouteResult proposal{Outcome::Forwarded,true,true,false};
    std::array<uint64_t,static_cast<unsigned>(AdmissionBlock::Count)> admissionChecks{};
    uint64_t boundaryQueued=0, boundaryQueueFailed=0, boundaryNotReady=0;
    uint64_t mapRefreshDeferred=0, mapRefreshFailed=0, mapRefreshSucceeded=0;
    bool mapsCompatible = true;
    bool admitted = false;
    bool knownAdmission = false;
    bool consumerRouteInvoked = false;
    LockPhase phase;
    bool revoking = false;
    unsigned registeredCount = 0;
    uint64_t modeledEvents = 0, unmappedEvents = 0, keymapChanges = 0, correlatedAdmissions = 0;
    void* lifecycleContext = nullptr;
    void (*lifecycle)(void*,unsigned,uint64_t,unsigned) = nullptr;
    void announce(unsigned opcode,uint64_t device,unsigned reason) {
        if (lifecycle) lifecycle(lifecycleContext,opcode,device,reason);
    }
    explicit ResearchRoute(StockHooks& stock) : output(stock) {}
    ~ResearchRoute() { if (map) xkb_keymap_unref(map); }
    void retire(SP<IKeyboard> keyboard) {
        for (auto& slot : registered) {
            if (!slot.generation || slot.keyboard.lock()!=keyboard) continue;
            const auto generation=slot.generation;
            devices.detach(keyboard.get(),router,output);
            slot.generation=0;slot.keyboard.reset();
            --registeredCount;
            announce(8,generation,1);
        }
    }
    bool releasedForLayout() const {
        if (!router.quiescent() || phase.routeLocked() || current || revoking) return false;
        for (const auto& keyboard : g_pInputManager->m_keyboards) {
            if (!keyboard || !keyboard->m_enabled || !keyboard->m_allowed) continue;
            for (uint32_t key=0;key<keyCount;++key) if (keyboard->getPressed(key)) return false;
        }
        return true;
    }
    bool rebuildEquivalentMaps() {
        if (deferMapRefresh(router.quiescent(),phase.routeLocked(),current.has_value(),revoking)) {
            ++mapRefreshDeferred;
            return false; // defer, do not poison an unchanged proven map
        }
        mapsCompatible=false;
        ++mapRefreshFailed;
        auto active=g_pSeatManager->m_keyboard.lock();
        if (!active || !active->m_xkbKeymap) return false;
        const auto aq=active->aq();
        auto* handle=aq ? aq->getLibinputHandle() : nullptr;
        if (active->isVirtual() || !active->m_enabled || !active->m_allowed || !handle ||
            libinput_device_keyboard_has_key(handle,KEY_A)<=0 ||
            libinput_device_keyboard_has_key(handle,KEY_LEFTSHIFT)<=0) return false;
        for (const auto& keyboard : g_pInputManager->m_keyboards) {
            if (!keyboard || !keyboard->m_enabled || !keyboard->m_allowed) continue;
            // Consumer-map compatibility is proved here, never by ORing masks.
            if (keyboard->isVirtual() || !equivalentKeymap(active->m_xkbKeymap,keyboard->m_xkbKeymap)) return false;
            for (uint32_t key=0;key<keyCount;++key) if (keyboard->getPressed(key)) return false;
        }
        if (!forwarded.setKeymap(active->m_xkbKeymap)) return false;
        auto* next=xkb_keymap_ref(active->m_xkbKeymap);
        if (map) xkb_keymap_unref(map);
        map=next;
        mapsCompatible=true;
        --mapRefreshFailed;++mapRefreshSucceeded;
        return true;
    }
    // Lifecycle only: existing ownership is never erased to admit a new map.
    void registerExisting(bool initial=true) {
        if (!map) {
            // Seat consumer, not enumeration order, owns aggregate XKB indices.
            auto active=g_pSeatManager->m_keyboard.lock();
            const auto aq=active ? active->aq() : nullptr;
            auto* handle=aq ? aq->getLibinputHandle() : nullptr;
            auto* selected=consumerKeymap(active ? active->m_xkbKeymap : nullptr,
                active && !active->isVirtual() && handle,
                active && active->m_enabled,active && active->m_allowed,
                handle && libinput_device_keyboard_has_key(handle,KEY_A)>0,
                handle && libinput_device_keyboard_has_key(handle,KEY_LEFTSHIFT)>0);
            if (!selected) throw std::runtime_error("research requires verified active typing consumer map");
            map=xkb_keymap_ref(selected);
            if (!forwarded.setKeymap(map)) throw std::runtime_error("research XKB allocation failed");
        }
        for (const auto& keyboard : g_pInputManager->m_keyboards) {
            if (!keyboard || keyboard->isVirtual() || !keyboard->m_enabled || !keyboard->m_allowed) continue;
            bool existing=false;
            for (auto& slot : registered) {
                if (!slot.generation || slot.keyboard.lock()!=keyboard) continue;
                existing=true;
                if (slot.map!=keyboard->m_xkbKeymap && router.quiescent() &&
                    equivalentKeymap(map,keyboard->m_xkbKeymap)) slot.map=keyboard->m_xkbKeymap;
            }
            if (existing) continue;
            const auto aq=keyboard->aq();
            auto* handle=aq ? aq->getLibinputHandle() : nullptr;
            if (!handle || !libinput_device_has_capability(handle,LIBINPUT_DEVICE_CAP_KEYBOARD) || !keyboard->m_xkbKeymap) continue;
            // Physical Aquamarine/libinput key events are evdev codes; virtual
            // keyboard fixture code 1 is deliberately never admitted here.
            // Account for every verified physical keyboard, including power/
            // media-only devices: excluding them here but demanding complete
            // registry coverage at reason8 made neutral VM admission impossible.
            if (map && !equivalentKeymap(map,keyboard->m_xkbKeymap)) continue;
            bool held=false;
            for (uint32_t key=0;key<keyCount;++key) if (keyboard->getPressed(key)) held=true;
            if (held) {
                if (initial) throw std::runtime_error("research load requires released physical keyboard");
                continue;
            }
            std::array<uint32_t,keyCount> translation{};
            translation.fill(PhysicalDevices::unmapped);
            for (uint32_t key=0; key<keyCount; ++key) {
                if (libinput_device_keyboard_has_key(handle,key)>0 && xkb_keymap_key_get_name(keyboard->m_xkbKeymap,key+8)) {
                    if (keyboard->getPressed(key)) throw std::runtime_error("research load requires released physical keyboard");
                    translation[key]=key;
                }
            }
            for (auto& slot : registered) {
                if (slot.generation) continue;
                const auto id=devices.attach(keyboard.get(),true,false,translation,router);
                if (!id) break;
                ++registeredCount;
                Debug::log(LOG,"SLEDGE_RESEARCH_REGISTER device={} generation={} name={} map={} physical=true",static_cast<void*>(keyboard.get()),id,keyboard->m_hlName,static_cast<void*>(keyboard->m_xkbKeymap));
                slot.keyboard=keyboard; slot.map=keyboard->m_xkbKeymap; slot.generation=id;
                break;
            }
        }
        // Fixture must load before held input. Lock state is read on key entry,
        // never inferred from focus or destruction. No matcher capture enabled.
        if (!initial) return;
        if (!registeredCount) throw std::runtime_error("research fixture has no verified physical keyboard");
        if (PROTO::sessionLock->isLocked()) throw std::runtime_error("research load requires unlocked released fixture");
        router.unlockConfirmed();
    }
    std::optional<Event> event(SP<IKeyboard> keyboard, uint32_t raw, bool down) {
        for (const auto& slot : registered) {
            if (!slot.generation || slot.keyboard.lock()!=keyboard) continue;
            if (slot.map!=keyboard->m_xkbKeymap || !keyboard->m_allowed || !keyboard->m_enabled) {
                ++keymapChanges;
                revoke();
                return {}; // no allocation/keymap serialization in callbacks
            }
            auto result=devices.event(keyboard.get(),raw,down);
            if (result) ++modeledEvents; else ++unmappedEvents;
            return result;
        }
        ++unmappedEvents;
        return {};
    }
    bool consumerMapKnown() const {
        if (!mapsCompatible) return false;
        auto active=g_pSeatManager->m_keyboard.lock();
        if (!active) return false;
        for (const auto& slot : registered)
            if (slot.generation && slot.keyboard.lock()==active && slot.map==active->m_xkbKeymap) return true;
        return false;
    }
    AdmissionBlock admissionBlock(const IKeyboard* entering,uint32_t enteringKey) const {
        if (!mapsCompatible) return AdmissionBlock::MapsIncompatible;
        auto active=g_pSeatManager->m_keyboard.lock();
        if (!active) return AdmissionBlock::NoConsumer;
        if (!consumerMapKnown()) return AdmissionBlock::ConsumerUnregistered;
        if (!router.quiescent()) return AdmissionBlock::RouterBusy;
        if (phase.routeLocked()) return AdmissionBlock::Locked;
        if (current) return AdmissionBlock::Nested;
        if (revoking) return AdmissionBlock::Revoking;
        for (const auto& slot : registered) {
            auto keyboard=slot.keyboard.lock();
            if (!slot.generation || !keyboard) continue;
            if (slot.map!=keyboard->m_xkbKeymap || !keyboard->m_enabled || !keyboard->m_allowed) return AdmissionBlock::DeviceChanged;
            for (uint32_t key=0;key<keyCount;++key)
                if (preEventHold(keyboard->getPressed(key),keyboard.get(),key,entering,enteringKey)) return AdmissionBlock::PhysicalHold;
        }
        for (const auto& keyboard : g_pInputManager->m_keyboards) {
            if (!keyboard || !keyboard->m_enabled || !keyboard->m_allowed) continue;
            bool accounted=false;
            for (const auto& slot : registered) if (slot.generation && slot.keyboard.lock()==keyboard) accounted=true;
            if (!accounted) return AdmissionBlock::Unaccounted;
        }
        return AdmissionBlock::Ready;
    }
    bool physicalQuiescent(const IKeyboard* entering=nullptr,uint32_t enteringKey=keyCount) {
        const auto block=admissionBlock(entering,enteringKey);
        ++admissionChecks[static_cast<unsigned>(block)];
        return block==AdmissionBlock::Ready;
    }
    void revoke() {
        phase.enter();
        revoking=true;
        router.lock(output); // old focus/IME route, BEFORE stock original/focus/ACK
        (void)forwarded.synchronize(router);
        revoking=false;
        announce(9,0,1);
    }
    void syncUnlock(bool locked) {
        if (phase.observe(locked)) {
            router.unlockConfirmed();
            (void)forwarded.synchronize(router); // locker holds cannot leak to app enter
            announce(9,0,0);
        }
    }
};
} // namespace sledge::hyprland
