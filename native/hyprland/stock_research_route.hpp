#pragma once
// Compiled only into explicit isolated research target; no production admission.
#include "stock_hooks.hpp"
#include "physical_devices.hpp"
#include "forwarded_keyboard_state.hpp"
#include "keymap_equivalence.hpp"
#include "lock_phase.hpp"
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
    HookTransaction<CFunctionHook,4> transaction;
    std::array<CFunctionHook*,4> hooks{};
    std::array<HookBytes,4> bytes{};
public:
    ~ResearchHooks() { reset(); }
    void install(HANDLE owner, const std::array<void*,4>& destinations) {
        const std::array<std::string,4> names{"sendEnter","sendMods","onKeyEvent","updateCapabilities"};
        const std::array<std::string,4> signatures{
            "CWLKeyboardResource::sendEnter(Hyprutils::Memory::CSharedPointer<CWLSurfaceResource>, wl_array*)",
            "CWLKeyboardResource::sendMods(unsigned int, unsigned int, unsigned int, unsigned int)",
            "CKeybindManager::onKeyEvent(std::any, Hyprutils::Memory::CSharedPointer<IKeyboard>)",
            "CWLSeatProtocol::updateCapabilities(unsigned int)"};
        std::array<void*,4> sources{};
        for (unsigned i=0; i<4; ++i) {
            for (const auto& match : HyprlandAPI::findFunctionsByName(owner,names[i])) {
                if (match.demangled!=signatures[i]) continue;
                if (sources[i] || !match.address) throw std::runtime_error("ambiguous research signature");
                sources[i]=match.address;
            }
            if (!sources[i] || !destinations[i]) throw std::runtime_error("missing research signature: "+signatures[i]);
            bytes[i].snapshot(sources[i]);
        }
        try {
            for (unsigned i=0; i<4; ++i) {
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
        CHyprSignalListener destroy;
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
    bool admitted = false;
    bool knownAdmission = false;
    bool consumerRouteInvoked = false;
    LockPhase phase;
    bool revoking = false;
    unsigned registeredCount = 0;
    uint64_t modeledEvents = 0, unmappedEvents = 0, keymapChanges = 0, correlatedAdmissions = 0;
    explicit ResearchRoute(StockHooks& stock) : output(stock) {}
    // Lifecycle allocation/verification occurs only at initialization. No lazy
    // discovery or listener creation from the key callback. New devices bypass.
    void registerExisting() {
        for (const auto& keyboard : g_pInputManager->m_keyboards) {
            if (!keyboard || keyboard->isVirtual() || !keyboard->m_enabled || !keyboard->m_allowed) continue;
            const auto aq=keyboard->aq();
            auto* handle=aq ? aq->getLibinputHandle() : nullptr;
            if (!handle || !libinput_device_has_capability(handle,LIBINPUT_DEVICE_CAP_KEYBOARD) || !keyboard->m_xkbKeymap) continue;
            // Physical Aquamarine/libinput key events are evdev codes; virtual
            // keyboard fixture code 1 is deliberately never admitted here.
            // Ignore power/media-only devices before choosing the aggregate map.
            if (libinput_device_keyboard_has_key(handle,KEY_A)<=0 ||
                libinput_device_keyboard_has_key(handle,KEY_LEFTSHIFT)<=0) continue;
            if (map && !equivalentKeymap(map,keyboard->m_xkbKeymap)) continue;
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
                Debug::log(LOG,"SLEDGE_RESEARCH_REGISTER device={} generation={} name={} map={} typing=true",static_cast<void*>(keyboard.get()),id,keyboard->m_hlName,static_cast<void*>(keyboard->m_xkbKeymap));
                slot.keyboard=keyboard; slot.map=keyboard->m_xkbKeymap; slot.generation=id;
                slot.destroy=keyboard->m_events.destroy.listen([this,identity=keyboard.get(),&slot] {
                    devices.detach(identity,router,output); slot.generation=0; slot.keyboard.reset();
                });
                if (!map) { map=slot.map; if (!forwarded.setKeymap(map)) throw std::runtime_error("research XKB allocation failed"); }
                break;
            }
        }
        // Fixture must load before held input. Lock state is read on key entry,
        // never inferred from focus or destruction. No matcher capture enabled.
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
    void revoke() {
        phase.enter();
        revoking=true;
        router.lock(output); // old focus/IME route, BEFORE stock original/focus/ACK
        (void)forwarded.synchronize(router);
        revoking=false;
    }
    void syncUnlock(bool locked) {
        if (phase.observe(locked)) {
            router.unlockConfirmed();
            (void)forwarded.synchronize(router); // locker holds cannot leak to app enter
        }
    }
};
} // namespace sledge::hyprland
