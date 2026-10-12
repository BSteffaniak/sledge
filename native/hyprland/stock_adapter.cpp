// Concrete stock adapter: default is passive, all original routes unchanged.
// SLEDGE_STOCK_RESEARCH adds explicit isolated lock-quarantine fixture only.
// Both advertise capabilities zero; no replacement or production admission.
#include "stock_hooks.hpp"
#include "stock_worker.hpp"
#include "physical_devices.hpp"
#include "forwarded_keyboard_state.hpp"
#include "isolated_output.hpp"
#include <hyprland/src/managers/input/InputManager.hpp>
#include <hyprland/src/managers/SeatManager.hpp>
#include <hyprland/src/protocols/InputMethodV2.hpp>
#include <hyprland/src/protocols/SessionLock.hpp>
#include <sys/random.h>
#include <memory>
#include <cstdlib>
#include <any>
#ifdef SLEDGE_STOCK_RESEARCH
#include "stock_research_route.hpp"
#include "stock_replacement.hpp"
#endif

namespace {
using namespace sledge::hyprland;
// Unsupported replacement cannot emit anything or masquerade as Swallow.
struct PassiveOutput : Output {
    Delivery replace(uint32_t, uint8_t, uint8_t) override { return Delivery::RejectedNoOutput; }
    bool release(uint32_t) override { return false; }
    bool reconcileModifiers(uint8_t) override { return false; }
};
struct Adapter {
    StockHooks hooks;
    Router router;
    PhysicalDevices devices;
    ForwardedKeyboardState forwarded;
    PassiveOutput output;
    DecisionMailbox mailbox;
    std::unique_ptr<StockWorker> worker;
#ifdef SLEDGE_STOCK_RESEARCH
    std::unique_ptr<ResearchRoute> research;
    StockReplacement replacement;
    bool replacementWire = false;
    uint64_t nonce = 0, wireSequence = 0, wireEpoch = 0;
    WirePacket eventPacket{};
    struct TransactionOutput : Output {
        Adapter& adapter;
        explicit TransactionOutput(Adapter& value) : adapter(value) {}
        Delivery replace(uint32_t key,uint8_t requested,uint8_t) override {
            return adapter.replacement.replace(key,requested,*adapter.research);
        }
        bool release(uint32_t key) override { return adapter.research->output.release(key); }
        bool reconcileModifiers(uint8_t sides) override { return adapter.research->output.reconcileModifiers(sides); }
    };
#endif
    explicit Adapter(HANDLE handle) : hooks(handle) {}
};
std::unique_ptr<Adapter> adapter;
void inputKey(CInputManager* self, const IKeyboard::SKeyEvent& event, SP<IKeyboard> keyboard) {
#ifdef SLEDGE_STOCK_RESEARCH
    auto* research=adapter->research.get();
    if (research && !research->current) {
        research->syncUnlock(PROTO::sessionLock->isLocked());
        auto modeled=research->event(keyboard,event.keycode,event.state==WL_KEYBOARD_KEY_STATE_PRESSED);
        if (modeled) {
            research->output.time=event.timeMs;
            research->current=modeled;
            research->currentKeyboard=keyboard.get();
            research->admitted=false;
            research->knownAdmission=false;
            research->consumerRouteInvoked=false;
            std::optional<Ticket> ticket;
            std::optional<Decision> reply;
            Adapter::TransactionOutput transaction(*adapter);
            if (adapter->replacementWire && adapter->worker && adapter->worker->connected() && !research->phase.routeLocked()) {
                ticket=modeled->state==State::Down ? research->router.begin(*modeled,monotonicNs(),10'000'000) :
                    research->router.settlement(*modeled,monotonicNs(),10'000'000);
                if (ticket) {
                    if (ticket->epoch!=adapter->wireEpoch) {
                        WirePacket reset{}; reset[0]='S';reset[1]='H';reset[2]='P';reset[3]='1';
                        wire::put(reset,4,2,1);wire::put(reset,6,2,9);wire::put(reset,8,4,8);
                        wire::put(reset,24,8,ticket->epoch);wire::put(reset,32,8,++adapter->wireSequence);
                        wire::put(reset,40,8,ticket->deadline);wire::put(reset,48,8,adapter->nonce);
                        wire::put(reset,64,4,0);
                        if (!adapter->worker->notify(reset)) ticket.reset();
                        else adapter->wireEpoch=wire::get(reset,24,8);
                    }
                }
                if (ticket) {
                    auto& packet=adapter->eventPacket; packet={};
                    packet[0]='S'; packet[1]='H'; packet[2]='P'; packet[3]='1';
                    wire::put(packet,4,2,1); wire::put(packet,6,2,3); wire::put(packet,8,4,32);
                    wire::put(packet,16,8,ticket->device); wire::put(packet,24,8,ticket->epoch);
                    wire::put(packet,32,8,++adapter->wireSequence); wire::put(packet,40,8,ticket->deadline); wire::put(packet,48,8,adapter->nonce);
                    wire::put(packet,64,4,modeled->key); wire::put(packet,68,4,static_cast<unsigned>(modeled->state));
                    wire::put(packet,72,4,event.timeMs); wire::put(packet,76,4,research->router.forwardedModifiers()); wire::put(packet,80,8,ticket->focus);
                    WirePacket answer{};
                    // Reserve 1ms of the aggregate budget for compositor commit.
                    if (adapter->mailbox.decide(packet,ticket->deadline-1'000'000,answer) && wire::decision(packet,answer))
                        if (modeled->state==State::Down)
                            reply=Decision{*ticket,static_cast<Verdict>(wire::get(answer,64,4)),static_cast<uint32_t>(wire::get(answer,68,4)),static_cast<uint8_t>(wire::get(answer,72,4))};
                    adapter->replacement.deadline=ticket->deadline;
                }
            }
            research->proposal=research->router.route(*modeled,ticket,reply,monotonicNs(),adapter->replacementWire ? static_cast<Output&>(transaction) : static_cast<Output&>(research->output));
            // Consumed/quarantined repeats/up never enter stock. StockConsumed
            // still enters the original keybind path for release/timer cleanup.
            const bool quarantinedCleanup=modeled->state==State::Up && research->proposal.outcome==Outcome::Quarantined;
            if (quarantinedCleanup) {
                // Pre-lock keys entered InputManager::m_pressed. Keybind-only
                // cleanup leaves that private ledger stuck and suppresses the
                // next fresh down. Run the complete original release path with
                // client emission denied by proposal, including when locked.
                reinterpret_cast<void (*)(CInputManager*,const IKeyboard::SKeyEvent&,SP<IKeyboard>)>(adapter->hooks.original(StockHooks::InputKey))(self,event,keyboard);
            } else if (research->phase.routeLocked()) {
                // Stock InputManager selects the IME before seat delivery, even
                // while locked. Keep keybind cleanup, but fresh locker ownership
                // must use the authoritative seat route, never the app IME grab.
                if (research->proposal.processStock || quarantinedCleanup) {
                    research->admitted=reinterpret_cast<bool (*)(CKeybindManager*,std::any,SP<IKeyboard>)>(research->hooks.original(2))
                        (g_pKeybindManager.get(),std::any(event),keyboard);
                    research->knownAdmission=true;
                    research->proposal=research->router.originalOutcome(*modeled,research->proposal,research->admitted);
                }
                (void)research->forwarded.synchronize(research->router,true);
                if (research->proposal.deliverClient && research->admitted) {
                    g_pSeatManager->setKeyboard(keyboard);
                    reinterpret_cast<void (*)(CSeatManager*,uint32_t,uint32_t,wl_keyboard_key_state)>(adapter->hooks.original(StockHooks::SeatKey))
                        (g_pSeatManager.get(),event.timeMs,event.keycode,event.state);
                    const auto& state=research->forwarded;
                    reinterpret_cast<void (*)(CSeatManager*,uint32_t,uint32_t,uint32_t,uint32_t)>(adapter->hooks.original(StockHooks::SeatMods))
                        (g_pSeatManager.get(),state.depressed(),state.latched(),state.locked(),state.group());
                }
            } else if (research->proposal.processStock || quarantinedCleanup) {
                reinterpret_cast<void (*)(CInputManager*,const IKeyboard::SKeyEvent&,SP<IKeyboard>)>(adapter->hooks.original(StockHooks::InputKey))(self,event,keyboard);
            }
            if (research->knownAdmission)
                research->router.originalOutcome(*modeled,research->proposal,research->admitted);
            (void)research->forwarded.synchronize(research->router,research->phase.routeLocked());
            if (ticket && adapter->worker) {
                auto outcome=adapter->eventPacket;
                for (unsigned i=64;i<512;++i) outcome[i]=0;
                wire::put(outcome,6,2,5); wire::put(outcome,8,4,24);
                const auto result=research->proposal.outcome;
                unsigned code=2; // uncertain, never infer actual original admission
                if (result==Outcome::Replaced) code=0;
                else if (result==Outcome::Swallowed) code=4;
                else if (result==Outcome::StockConsumed) code=5;
                else if (result==Outcome::Merged) code=7;
                else if (result==Outcome::LockerOwned) code=8;
                else if (result==Outcome::Quarantined) code=9;
                // Original admission requires actual stock keybind correlation.
                else if (research->knownAdmission && research->admitted && research->consumerRouteInvoked) code=result==Outcome::Rejected ? 1 : (modeled->state!=State::Down ? 6 : (!reply ? 3 : 6));
                wire::put(outcome,64,4,code); wire::put(outcome,72,4,modeled->key);
                wire::put(outcome,76,4,static_cast<unsigned>(modeled->state)); wire::put(outcome,80,8,ticket->focus);
                if (!adapter->worker->notify(outcome)) research->router.disconnect(research->output);
            }
            research->current.reset();
            research->currentKeyboard=nullptr;
            return;
        }
    }
#endif
    // Unknown/virtual/unregistered devices never share physical identity.
    reinterpret_cast<void (*)(CInputManager*,const IKeyboard::SKeyEvent&,SP<IKeyboard>)>(adapter->hooks.original(StockHooks::InputKey))(self,event,keyboard);
}
void inputMods(CInputManager* self, SP<IKeyboard> keyboard) {
    reinterpret_cast<void (*)(CInputManager*,SP<IKeyboard>)>(adapter->hooks.original(StockHooks::InputMods))(self,keyboard);
}
void seatKey(CSeatManager* self, uint32_t time, uint32_t key, wl_keyboard_key_state state) {
#ifdef SLEDGE_STOCK_RESEARCH
    if (adapter->research) {
        if (adapter->research->current && !adapter->research->proposal.deliverClient) return;
        if (!adapter->research->revoking) {
            adapter->research->output.ime.reset();
            adapter->research->output.seat=self->m_state.keyboardFocusResource;
            if (adapter->research->current && self->m_state.keyboardFocusResource.lock()) adapter->research->consumerRouteInvoked=true;
        }
    }
#endif
    reinterpret_cast<void (*)(CSeatManager*,uint32_t,uint32_t,wl_keyboard_key_state)>(adapter->hooks.original(StockHooks::SeatKey))(self,time,key,state);
}
void seatMods(CSeatManager* self, uint32_t depressed, uint32_t latched, uint32_t locked, uint32_t group) {
#ifdef SLEDGE_STOCK_RESEARCH
    if (adapter->research) {
        const auto& state=adapter->research->forwarded;
        depressed=state.depressed(); latched=state.latched(); locked=state.locked(); group=state.group();
    }
#endif
    reinterpret_cast<void (*)(CSeatManager*,uint32_t,uint32_t,uint32_t,uint32_t)>(adapter->hooks.original(StockHooks::SeatMods))(self,depressed,latched,locked,group);
}
void focus(CSeatManager* self, SP<CWLSurfaceResource> surface) {
#ifdef SLEDGE_STOCK_RESEARCH
    if (adapter->research) {
        adapter->research->syncUnlock(PROTO::sessionLock->isLocked());
        adapter->research->router.focusChanged();
    }
#endif
    adapter->router.focusChanged(); // speculation only, never stock focus state
    reinterpret_cast<void (*)(CSeatManager*,SP<CWLSurfaceResource>)>(adapter->hooks.original(StockHooks::Focus))(self,surface);
}
void imeKey(CInputMethodKeyboardGrabV2* self, uint32_t time, uint32_t key, wl_keyboard_key_state state) {
#ifdef SLEDGE_STOCK_RESEARCH
    if (adapter->research) {
        if (adapter->research->phase.routeLocked() && !adapter->research->revoking) return;
        if (!adapter->research->revoking) {
            adapter->research->output.ime=self->getOwner();
            if (adapter->research->current && self->good()) adapter->research->consumerRouteInvoked=true;
        }
        if (!adapter->research->revoking && adapter->research->current && !adapter->research->proposal.deliverClient) return;
    }
#endif
    reinterpret_cast<void (*)(CInputMethodKeyboardGrabV2*,uint32_t,uint32_t,wl_keyboard_key_state)>(adapter->hooks.original(StockHooks::ImeKey))(self,time,key,state);
}
void imeMods(CInputMethodKeyboardGrabV2* self, uint32_t depressed, uint32_t latched, uint32_t locked, uint32_t group) {
#ifdef SLEDGE_STOCK_RESEARCH
    if (adapter->research) {
        if (adapter->research->phase.routeLocked() && !adapter->research->revoking) return;
        const auto& state=adapter->research->forwarded;
        if (adapter->research->revoking) depressed=latched=locked=group=0;
        else { depressed=state.depressed(); latched=state.latched(); locked=state.locked(); group=state.group(); }
    }
#endif
    reinterpret_cast<void (*)(CInputMethodKeyboardGrabV2*,uint32_t,uint32_t,uint32_t,uint32_t)>(adapter->hooks.original(StockHooks::ImeMods))(self,depressed,latched,locked,group);
}
void lock(CSessionLockProtocol* self, CExtSessionLockManagerV1* manager, uint32_t id) {
    // Revocation is synchronous before original. There are no admitted devices
    // or forwarded holds in this passive build. It must not enable on unlock.
#ifdef SLEDGE_STOCK_RESEARCH
    if (adapter->research) adapter->research->revoke();
#endif
    adapter->router.lock(adapter->output);
    reinterpret_cast<void (*)(CSessionLockProtocol*,CExtSessionLockManagerV1*,uint32_t)>(adapter->hooks.original(StockHooks::LockEntry))(self,manager,id);
#ifdef SLEDGE_STOCK_RESEARCH
    if (adapter->research) adapter->research->phase.originalReturned(
        reinterpret_cast<bool (*)(CSessionLockProtocol*)>(adapter->hooks.original(StockHooks::LockQuery))(self));
#endif
}
bool locked(CSessionLockProtocol* self) {
    return reinterpret_cast<bool (*)(CSessionLockProtocol*)>(adapter->hooks.original(StockHooks::LockQuery))(self);
}
#ifdef SLEDGE_STOCK_RESEARCH
void capabilities(CWLSeatProtocol* self,uint32_t caps) {
    reinterpret_cast<void (*)(CWLSeatProtocol*,uint32_t)>(adapter->research->hooks.original(3))(self,caps);
    adapter->replacement.observedCapabilities=caps;
    adapter->replacement.capabilitiesKnown=true;
}
void enter(CWLKeyboardResource* self, SP<CWLSurfaceResource> surface, wl_array* original) {
    adapter->replacement.entered(self,surface);
    auto* research=adapter->research.get();
    // Stack-backed wl_array, no wl_array_add allocation. Stock sendEnter copies
    // synchronously. Only committed forwarded union is visible in any new focus.
    std::array<uint32_t,keyCount> keys{}; unsigned count=0;
    for (uint32_t key=0; key<keyCount; ++key)
        if (research->forwarded.focusEnterKeys()[key]) keys[count++]=key;
    wl_array filtered{count*sizeof(uint32_t),keys.size()*sizeof(uint32_t),keys.data()};
    (void)original;
    reinterpret_cast<void (*)(CWLKeyboardResource*,SP<CWLSurfaceResource>,wl_array*)>(research->hooks.original(0))(self,surface,&filtered);
}
void resourceMods(CWLKeyboardResource* self, uint32_t, uint32_t, uint32_t, uint32_t) {
    if (adapter->research->revoking) {
        reinterpret_cast<void (*)(CWLKeyboardResource*,uint32_t,uint32_t,uint32_t,uint32_t)>(adapter->research->hooks.original(1))(self,0,0,0,0);
        return;
    }
    const auto& state=adapter->research->forwarded;
    reinterpret_cast<void (*)(CWLKeyboardResource*,uint32_t,uint32_t,uint32_t,uint32_t)>(adapter->research->hooks.original(1))
        (self,state.depressed(),state.latched(),state.locked(),state.group());
}
bool keybind(CKeybindManager* self, std::any event, SP<IKeyboard> keyboard) {
    auto& research=*adapter->research;
    // Stock ABI takes std::any BY VALUE and casts its contained SKeyEvent.
    // Inspect without copying/allocating; correlate only this physical callback.
    const auto* keyEvent=std::any_cast<IKeyboard::SKeyEvent>(&event);
    const bool correlated=keyEvent && research.current && research.currentKeyboard==keyboard.get() &&
        research.current->key==keyEvent->keycode &&
        (research.current->state!=State::Up)==(keyEvent->state==WL_KEYBOARD_KEY_STATE_PRESSED);
    const bool admitted=reinterpret_cast<bool (*)(CKeybindManager*,std::any,SP<IKeyboard>)>(research.hooks.original(2))(self,std::move(event),keyboard);
    if (correlated) {
        ++research.correlatedAdmissions;
        research.admitted=admitted; research.knownAdmission=true;
        research.proposal=research.router.originalOutcome(*research.current,research.proposal,admitted);
        (void)research.forwarded.synchronize(research.router,research.phase.routeLocked());
    }
    return admitted; // keep actual stock keybind release/timer semantics
}
#endif
}
APICALL EXPORT std::string PLUGIN_API_VERSION() { return HYPRLAND_API_VERSION; }
APICALL EXPORT void PLUGIN_EXIT() {
    if (!adapter) return;
#ifdef SLEDGE_STOCK_RESEARCH
    if (adapter->research) {
        const auto& research=*adapter->research;
        Debug::log(LOG,"SLEDGE_RESEARCH_EXIT registered={} modeled={} unmapped={} keymap_changes={} correlated_admissions={}",research.registeredCount,research.modeledEvents,research.unmappedEvents,research.keymapChanges,research.correlatedAdmissions);
        adapter->research->hooks.reset();
    }
#endif
    adapter->hooks.reset(); // verify byte restoration before worker join/unmap
    adapter.reset();
}
APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT(HANDLE handle) {
    if (adapter) throw std::runtime_error("passive stock adapter already loaded");
    std::array<char,4096> executable{};
    const auto length = readlink("/proc/self/exe",executable.data(),executable.size());
    if (length<=0 || static_cast<unsigned long>(length)>=executable.size())
        throw std::runtime_error("cannot identify stock runtime");
    adapter = std::make_unique<Adapter>(handle);
    try {
        adapter->hooks.installPassive(std::string_view(executable.data(),static_cast<std::size_t>(length)),
            __hyprland_api_get_hash(),__hyprland_api_get_client_hash(),
            {reinterpret_cast<void*>(&inputKey),reinterpret_cast<void*>(&inputMods),reinterpret_cast<void*>(&seatKey),
             reinterpret_cast<void*>(&seatMods),reinterpret_cast<void*>(&focus),reinterpret_cast<void*>(&imeKey),
             reinterpret_cast<void*>(&imeMods),reinterpret_cast<void*>(&lock),reinterpret_cast<void*>(&locked)});
#ifdef SLEDGE_STOCK_RESEARCH
        if (const auto* mode=std::getenv("SLEDGE_STOCK_RESEARCH_MODE")) {
            const auto selected=std::string_view(mode);
            if (selected!="lock-quarantine" && selected!="replacement-wire") throw std::runtime_error("unknown isolated research mode");
            adapter->replacementWire=selected=="replacement-wire";
            adapter->research=std::make_unique<ResearchRoute>(adapter->hooks);
            adapter->research->registerExisting();
            auto activeKeyboard=g_pSeatManager->m_keyboard.lock();
            if (!activeKeyboard || !adapter->replacement.setKeymap(activeKeyboard->m_xkbKeymap)) throw std::runtime_error("replacement keymap allocation failed");
            if (const auto* failure=std::getenv("SLEDGE_STOCK_RESEARCH_FAIL_AFTER")) {
                if (std::string_view(failure)=="preflight") adapter->replacement.rejectBefore=true;
                else if (std::string_view(failure)=="mods") adapter->replacement.failAfter=1;
                else if (std::string_view(failure)=="down") adapter->replacement.failAfter=2;
                else throw std::runtime_error("unknown replacement failure fixture");
            }
            adapter->research->hooks.install(handle,{reinterpret_cast<void*>(&enter),reinterpret_cast<void*>(&resourceMods),reinterpret_cast<void*>(&keybind),reinterpret_cast<void*>(&capabilities)});
        }
#endif
        // Explicit endpoint only; no default socket, scanning, credentials or
        // environment-based capture overrides. This connection sends Hello only.
        if (const auto* endpoint = std::getenv("SLEDGE_HYPRLAND_PASSIVE_SOCKET")) {
            uint64_t nonce = 0;
            if (getrandom(&nonce,sizeof(nonce),0)!=sizeof(nonce)) throw std::runtime_error("cannot create connection nonce");
#ifdef SLEDGE_STOCK_RESEARCH
            adapter->nonce=nonce;
#endif
            WirePacket hello{};
            constexpr std::string_view artifact = "stock-0.52.2:8dee6b387615f14e8ce6c1f7bbd0b58524c31d783061093423e6a3fc6f89b9ca";
            if (!wire::hello(hello,nonce,__hyprland_api_get_hash(),artifact)) throw std::runtime_error("passive Hello identity too long");
            adapter->worker = std::make_unique<StockWorker>(adapter->mailbox,endpoint,hello);
        }
    } catch (...) { PLUGIN_EXIT(); throw; }
    return {"sledge-stock-passive", "Stock ABI lifecycle/worker; capabilities zero, no capture", "sledge", "1"};
}
