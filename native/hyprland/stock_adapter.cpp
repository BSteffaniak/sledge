// Concrete stock adapter: default is passive, all original routes unchanged.
// SLEDGE_STOCK_RESEARCH adds explicit isolated lock-quarantine fixture only.
// Both advertise capabilities zero; no replacement or production admission.
#include <hyprland/src/managers/eventLoop/EventLoopManager.hpp>
#include "startup_output.hpp"
#include "output_reconciliation.hpp"
#include "drain_state.hpp"
#include "qualification_metadata.hpp"
#include "stock_hooks.hpp"
#if defined(SLEDGE_STOCK_PRODUCTION)
#define SLEDGE_STOCK_RESEARCH
static_assert(sledge::hyprland::validBuildId(),"production package must supply canonical build ID");
#endif
#if defined(SLEDGE_STOCK_CANDIDATE) && !defined(SLEDGE_STOCK_PRODUCTION)
#error "candidate requires production target"
#endif
#ifdef SLEDGE_STOCK_CANDIDATE
#include "candidate_gate.hpp"
#endif
#include "stock_worker.hpp"
#include "physical_devices.hpp"
#include "forwarded_keyboard_state.hpp"
#include "isolated_output.hpp"
#include <hyprland/src/managers/input/InputManager.hpp>
#include <hyprland/src/managers/SeatManager.hpp>
#include <hyprland/src/protocols/InputMethodV2.hpp>
#include <hyprland/src/protocols/core/Compositor.hpp>
#include <hyprland/src/protocols/SessionLock.hpp>
#include <sys/random.h>
#include <memory>
#include <cstdlib>
#include <any>
#ifdef SLEDGE_STOCK_RESEARCH
#include "stock_research_route.hpp"
#include "stock_replacement.hpp"
#include "focus_identity.hpp"
#include <hyprland/src/Compositor.hpp>
#include <hyprland/src/desktop/Window.hpp>
#include <hyprland/src/protocols/XDGShell.hpp>

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
    HANDLE ownerHandle;
    bool drainDispatcherInstalled=false;
    SP<CEventLoopTimer> registryTimer;
    StockPressedLedger stockLedger;
    DrainState drain;
    SP<SHyprCtlCommand> statusCommand;
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
    enum class CloseSource : unsigned { NewKeyboard, DestroyKeyboard, Layout, ConsumerChanged, OutcomeQueue, UnknownInput, Count };
    std::array<uint64_t,static_cast<unsigned>(CloseSource::Count)> closeSources{};
    uint64_t compatibleLifecycle=0;
    bool startupOutputPending=true;
    bool startupReplay=false;
    bool lifecycleDirty=false;
    bool initialReconciled = false;
    bool dropOutcomeOnce = false;
    uint64_t readyGeneration = 0;
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
    explicit Adapter(HANDLE handle) : ownerHandle(handle),hooks(handle) {}
};
std::unique_ptr<Adapter> adapter;
#ifdef SLEDGE_STOCK_RESEARCH
void reconcileStartupOutput() {
    if (!adapter->startupOutputPending || adapter->drain.active()) return;
    auto& research=*adapter->research;
    const bool physicalReleased=research.physicalQuiescent();
    const bool nativeReleased=g_pInputManager->getKeysFromAllKBs().empty();
    if (!physicalReleased || !nativeReleased) return;
    const auto surface=g_pSeatManager->m_state.keyboardFocus.lock();
    const auto owner=g_pSeatManager->m_state.keyboardFocusResource.lock();
    const auto keyboard=g_pSeatManager->m_keyboard.lock();
    if (!surface || !owner || !keyboard || owner->client()!=surface->client() || !owner->good()) return;
    const auto& mods=keyboard->m_modifiersState;
    if (mods.depressed) return;
    unsigned count=0;
    for (const auto& weak : owner->m_keyboards) {
        const auto resource=weak.lock();
        if (!resource) continue;
        if (!resource->good() || resource->m_owner.lock()!=owner) return;
        ++count;
    }
    if (!startupReplayAllowed(physicalReleased,nativeReleased,count!=0,mods.depressed!=0)) return;
    // Released native snapshot, NOT zeroing genuine Caps/latched history.
    research.forwarded.adoptReleasedNative(mods.latched,mods.locked,mods.group);
    adapter->startupReplay=true;
    wl_array keys{0,0,nullptr};
    for (const auto& weak : owner->m_keyboards) {
        const auto resource=weak.lock();
        if (!resource) continue;
        resource->sendEnter(surface,&keys);
        resource->sendMods(0,mods.latched,mods.locked,mods.group);
    }
    adapter->startupReplay=false;
    adapter->startupOutputPending=!adapter->replacement.recoveryBindingQualified(research);
}
void installRegistryRetry() {
    adapter->research->forEachMap([](xkb_keymap* map) { (void)adapter->replacement.prepare(map); });
    adapter->registryTimer=makeShared<CEventLoopTimer>(std::chrono::milliseconds(50),
        [](SP<CEventLoopTimer> timer,void*) {
            if (!adapter || adapter->drain.detached()) return;
            const auto previous=adapter->research->registeredCount;
            if (adapter->lifecycleDirty) {
                auto& research=*adapter->research;
                if (research.rebuildEquivalentMaps()) {
                    auto active=g_pSeatManager->m_keyboard.lock();
                    if (!active || !adapter->replacement.setKeymap(active->m_xkbKeymap)) research.revoke();
                    adapter->lifecycleDirty=false;
                }
            }
            adapter->research->registerExisting(false);
            adapter->research->forEachMap([](xkb_keymap* map) { (void)adapter->replacement.prepare(map); });
            auto active=g_pSeatManager->m_keyboard.lock();
            if (!adapter->research->projectionKnown && active && adapter->replacement.preparedFor(active->m_xkbKeymap)) {
                (void)adapter->research->selectConsumer(active);
                (void)adapter->replacement.select(active->m_xkbKeymap);
            }
            if (previous!=adapter->research->registeredCount) adapter->initialReconciled=false;
            reconcileStartupOutput();
            timer->updateTimeout(std::chrono::milliseconds(50));
        },nullptr);
    g_pEventLoopManager->addTimer(adapter->registryTimer);
}
bool nativeOutputReconciled(bool allowRetiredRecovery=false) {
    const auto& research=*adapter->research;
    bool held=false;
    for (const auto key : research.forwarded.focusEnterKeys()) held=held || key;
    return outputReconciled(adapter->stockLedger.empty(),research.consumerMapKnown(),held,
        research.forwarded.depressed()!=0,!allowRetiredRecovery && adapter->replacement.recovery.pending()) &&
        !adapter->lifecycleDirty && !adapter->startupOutputPending;
}
void installDrainControls(HANDLE handle) {
    if (!HyprlandAPI::addDispatcherV2(handle,"sledge:drain",[](std::string argument) {
            if (!argument.empty()) return SDispatchResult{false,false,"sledge:drain takes no arguments"};
            if (adapter->drain.detached()) return SDispatchResult{};
            adapter->drain.request();
            adapter->initialReconciled=false;
            adapter->research->router.focusChanged();
            return SDispatchResult{};
        })) throw std::runtime_error("cannot register drain dispatcher");
    adapter->drainDispatcherInstalled=true;
    adapter->statusCommand=HyprlandAPI::registerHyprCtlCommand(handle,SHyprCtlCommand{
        "sledge:status",true,[](eHyprCtlOutputFormat,std::string) {
            const char* blocked="none";
            if (!adapter->drain.detached()) {
                if (!adapter->drain.active()) blocked="not_draining";
                else if (!adapter->research->physicalQuiescent()) blocked="physical_or_output";
                else if (!nativeOutputReconciled()) blocked="output_unreconciled";
                else if (!adapter->mailbox.idle()) blocked="mailbox";
                else if (adapter->worker) {
                    adapter->worker->requestPark();
                    if (!adapter->worker->idleParked()) blocked="worker";
                }
                (void)adapter->drain.commit(std::string_view(blocked)=="none");
            }
            const bool ready=adapter->drain.detached();
            return std::string("{\"version\":1,\"draining\":")+(adapter->drain.active() ? "true" : "false")+
                ",\"quiescent\":"+(ready ? "true" : "false")+
                ",\"detached\":"+(ready ? "true" : "false")+",\"drain_blocked_reason\":\""+blocked+"\""+
                ",\"connection_generation\":"+std::to_string(adapter->readyGeneration)+
                ",\"ownership_epoch\":"+std::to_string(adapter->research->router.lockEpoch())+
                ",\"waiting_for_focus\":"+(adapter->startupOutputPending ? "true" : "false")+
                ",\"native_operational\":"+(!adapter->startupOutputPending && adapter->initialReconciled && adapter->worker && adapter->worker->connected() ? "true" : "false")+
                ",\"replacement_capabilities_known\":"+(adapter->replacement.capabilitiesKnown ? "true" : "false")+
                ",\"replacement_capabilities\":"+std::to_string(adapter->replacement.observedCapabilities)+
                ",\"replacement_preflight\":"+adapter->replacement.preflightStatus()+"}";
        }});
    if (!adapter->statusCommand) throw std::runtime_error("cannot register drain status");
}
bool lifecycle(void*,unsigned,uint64_t,unsigned);
bool ensureConnectionBinding() {
    if (adapter->drain.active()) return false;
    if (!adapter->worker || !adapter->worker->connected()) {
        if (adapter->initialReconciled) adapter->research->router.disconnect(adapter->research->output);
        adapter->initialReconciled=false;
        return false;
    }
    const auto generation=adapter->worker->generation();
    const auto nonce=adapter->worker->nonce();
    if (!adapter->worker->connected() || generation!=adapter->worker->generation()) return false;
    if (bindConnection(generation,nonce,adapter->readyGeneration,adapter->nonce,
                       adapter->wireSequence,adapter->wireEpoch)) {
        adapter->initialReconciled=false;
        // Binding is published before any ownership-preserving invalidation.
        adapter->research->router.disconnect(adapter->research->output);
    }
    return true;
}
void closeStream(Adapter::CloseSource source) {
    ++adapter->closeSources[static_cast<unsigned>(source)];
    adapter->initialReconciled=false;
    if (adapter->worker) adapter->worker->invalidate();
}
void beginLifecycle() {
    (void)ensureConnectionBinding();
    const bool previouslyReconciled=adapter->initialReconciled;
    adapter->initialReconciled=false;
    adapter->research->router.focusChanged(); // speculation only; ownership retained
    if (previouslyReconciled && !lifecycle(nullptr,7,0,3)) closeStream(Adapter::CloseSource::OutcomeQueue);
}
void finishLifecycle(Adapter::CloseSource) {
    adapter->lifecycleDirty=true;
}
void tryOutputRecovery() {
    if (adapter->drain.detached()) return;
    auto& research=*adapter->research;
    if (adapter->replacement.recovery.eligible(
            nativeOutputReconciled(true) && research.admissionBlock(nullptr,keyCount,true)==AdmissionBlock::Ready,
            research.consumerMapKnown() && adapter->replacement.recoveryBindingQualified(research),!research.phase.routeLocked()) &&
        research.router.recoverRetiredOutput(true)) {
        adapter->replacement.recovery.completed();
        adapter->initialReconciled=false;
    }
}
void selectKeyboard(CSeatManager* self,SP<IKeyboard> keyboard) {
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CSeatManager*,SP<IKeyboard>)>(adapter->research->hooks.original(8))(self,keyboard);return; }
    if (keyboard && self->m_keyboard.lock()!=keyboard) {
        if (!adapter->replacement.preparedFor(keyboard->m_xkbKeymap) ||
            !adapter->research->selectConsumer(keyboard) || !adapter->replacement.select(keyboard->m_xkbKeymap)) {
            adapter->research->projectionKnown=false;
            adapter->initialReconciled=false;
        }
    }
    reinterpret_cast<void (*)(CSeatManager*,SP<IKeyboard>)>(adapter->research->hooks.original(8))(self,keyboard);
}
void keyboardResourceDestroyed(CWLSeatProtocol* self,CWLKeyboardResource* resource) {
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CWLSeatProtocol*,CWLKeyboardResource*)>(adapter->research->hooks.original(7))(self,resource); return; }

    using Original=void(*)(CWLSeatProtocol*,CWLKeyboardResource*);
    adapter->replacement.recovery.retired(resource);
    reinterpret_cast<Original>(adapter->research->hooks.original(7))(self,resource);
}
void newKeyboard(CInputManager* self,SP<IKeyboard> keyboard) {
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CInputManager*,SP<IKeyboard>)>(adapter->research->hooks.original(4))(self,keyboard); return; }

    using Original=void(*)(CInputManager*,SP<IKeyboard>);
    beginLifecycle();
    reinterpret_cast<Original>(adapter->research->hooks.original(4))(self,keyboard);
    finishLifecycle(Adapter::CloseSource::NewKeyboard);
}
void destroyKeyboard(CInputManager* self,SP<IKeyboard> keyboard) {
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CInputManager*,SP<IKeyboard>)>(adapter->research->hooks.original(5))(self,keyboard); return; }

    using Original=void(*)(CInputManager*,SP<IKeyboard>);
    beginLifecycle();
    adapter->research->retire(keyboard);
    reinterpret_cast<Original>(adapter->research->hooks.original(5))(self,keyboard);
    finishLifecycle(Adapter::CloseSource::DestroyKeyboard);
}
void setKeyboardLayout(CInputManager* self) {
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CInputManager*)>(adapter->research->hooks.original(6))(self); return; }

    using Original=void(*)(CInputManager*);
    if (!adapter->research->releasedForLayout()) return;
    beginLifecycle();
    reinterpret_cast<Original>(adapter->research->hooks.original(6))(self);
    finishLifecycle(Adapter::CloseSource::Layout);
}
bool lifecycle(void*,unsigned opcode,uint64_t device,unsigned reason) {
    if (adapter->drain.active()) return false;
    if (!adapter->replacementWire || !ensureConnectionBinding()) return false;
    auto& research=*adapter->research;
    const auto epoch=research.router.lockEpoch();
    auto emit=[&](unsigned operation,uint64_t generation,unsigned value) {
        WirePacket packet{};
        if (!wire::control(packet,static_cast<wire::Opcode>(operation),
            {generation,epoch,++adapter->wireSequence,monotonicNs()+10'000'000,adapter->nonce},value)) return false;
        return adapter->worker->notify(packet);
    };
    // Frozen v1 uses Lock as the sole epoch-advancing control; never fake
    // unlock during Entering. Additional reset/retire follows the same epoch.
    if (epoch!=adapter->wireEpoch) {
        if (!emit(9,0,research.phase.routeLocked() ? 1 : 0)) return false;
        adapter->wireEpoch=epoch;
    }
    if (opcode!=9) return emit(opcode,device,reason);
    return true;
}
void focusedIdentity(WirePacket& packet) {
    const auto surface=g_pSeatManager->m_state.keyboardFocus.lock();
    if (!surface || adapter->research->phase.routeLocked()) return;
    const auto window=g_pCompositor->getWindowFromSurface(surface);
    if (!window) return;
    if (window->m_isX11) {
        // CWindow::m_class is stock's cached fetchClass(): XWayland appid
        // (WM_CLASS), updated by stock metadata handling. Surface lookup above
        // proves this is the actual focused window, not m_lastWindow polling.
        (void)encodeFocus(packet,2,window->m_class);
    } else {
        auto xdg=window->m_xdgSurface.lock();
        auto toplevel=xdg ? xdg->m_toplevel.lock() : nullptr;
        if (xdg && toplevel && xdg->m_surface.lock()==surface) (void)encodeFocus(packet,1,toplevel->m_state.appid);
    }
}
#endif
void inputKey(CInputManager* self, const IKeyboard::SKeyEvent& event, SP<IKeyboard> keyboard) {
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CInputManager*,const IKeyboard::SKeyEvent&,SP<IKeyboard>)>(adapter->hooks.original(StockHooks::InputKey))(self,event,keyboard); return; }

#ifdef SLEDGE_STOCK_RESEARCH
    auto* research=adapter->research.get();
    if (research && !research->current) {
        research->syncUnlock(PROTO::sessionLock->isLocked());
        if (adapter->initialReconciled && !research->consumerMapKnown() && adapter->worker) {
            closeStream(Adapter::CloseSource::ConsumerChanged);
        }
        (void)ensureConnectionBinding();
        tryOutputRecovery();
        if (adapter->drain.mayPropose() && adapter->replacementWire && !adapter->initialReconciled) {
            if (!adapter->worker || !adapter->worker->connected()) ++research->boundaryNotReady;
            else if (nativeOutputReconciled() && research->physicalQuiescent(keyboard.get(),event.state==WL_KEYBOARD_KEY_STATE_PRESSED ? event.keycode : keyCount)) {
                adapter->initialReconciled=lifecycle(nullptr,7,0,8);
                if (adapter->initialReconciled) ++research->boundaryQueued;
                else ++research->boundaryQueueFailed;
            }
        }
        const auto preConsumer=g_pSeatManager->m_keyboard.lock();
        if (preConsumer!=keyboard && adapter->initialReconciled) beginLifecycle();
        auto modeled=research->event(keyboard,event.keycode,event.state==WL_KEYBOARD_KEY_STATE_PRESSED);
        if (modeled) {
            // Replacement can precede stock setKeyboard; project the target
            // consumer's prepared map before modifier preflight, never allocate.
            if (adapter->replacement.preparedFor(keyboard->m_xkbKeymap)) {
                (void)research->selectConsumer(keyboard);
                (void)adapter->replacement.select(keyboard->m_xkbKeymap);
            } else research->projectionKnown=false;
            research->output.time=event.timeMs;
            research->current=modeled;
            research->currentKeyboard=keyboard.get();
            research->admitted=false;
            research->knownAdmission=false;
            research->consumerRouteInvoked=false;
            std::optional<Ticket> ticket;
            std::optional<Decision> reply;
            Adapter::TransactionOutput transaction(*adapter);
            const auto activeConsumer=g_pSeatManager->m_keyboard.lock();
            const bool consumerAlreadySelected=activeConsumer==keyboard;
            if (adapter->drain.mayPropose() && consumerAlreadySelected && adapter->replacementWire && adapter->worker && adapter->worker->connected() && adapter->initialReconciled && research->consumerMapKnown() && !research->phase.routeLocked()) {
                ticket=modeled->state==State::Down ? research->router.begin(*modeled,monotonicNs(),10'000'000) :
                    research->router.settlement(*modeled,monotonicNs(),10'000'000);
                if (ticket) {
                    if (ticket->epoch!=adapter->wireEpoch) {
                        WirePacket epochAdvance{};
                        const bool encoded=wire::control(epochAdvance,wire::Opcode::Lock,
                            {0,ticket->epoch,++adapter->wireSequence,ticket->deadline,adapter->nonce},0);
                        if (!encoded || !adapter->worker->notify(epochAdvance)) ticket.reset();
                        else adapter->wireEpoch=wire::get(epochAdvance,24,8);
                    }
                }
                if (ticket) {
                    auto& packet=adapter->eventPacket;
                    (void)wire::eventPacket(packet,{ticket->device,ticket->epoch,++adapter->wireSequence,ticket->deadline,adapter->nonce},
                        modeled->key,static_cast<unsigned>(modeled->state),event.timeMs,research->router.forwardedModifiers(),ticket->focus);
                    focusedIdentity(packet);
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
                if (adapter->dropOutcomeOnce && modeled->state==State::Down && reply && reply->verdict==Verdict::Replace) {
                    adapter->dropOutcomeOnce=false; // no replay/ownership reset
                } else if (!adapter->worker->notify(outcome)) {
                    research->router.disconnect(research->output);
                    closeStream(Adapter::CloseSource::OutcomeQueue);
                }
            }
            research->current.reset();
            tryOutputRecovery();
            research->currentKeyboard=nullptr;
                return;
        }
        if (adapter->initialReconciled && adapter->worker) {
            closeStream(Adapter::CloseSource::UnknownInput);
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
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CSeatManager*,uint32_t,uint32_t,wl_keyboard_key_state)>(adapter->hooks.original(StockHooks::SeatKey))(self,time,key,state); return; }

    adapter->stockLedger.seatRoute(key,state==WL_KEYBOARD_KEY_STATE_PRESSED);
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
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CSeatManager*,uint32_t,uint32_t,uint32_t,uint32_t)>(adapter->hooks.original(StockHooks::SeatMods))(self,depressed,latched,locked,group); return; }

#ifdef SLEDGE_STOCK_RESEARCH
    if (adapter->research && !adapter->startupOutputPending) {
        const auto& state=adapter->research->forwarded;
        depressed=state.depressed(); latched=state.latched(); locked=state.locked(); group=state.group();
    }
#endif
    reinterpret_cast<void (*)(CSeatManager*,uint32_t,uint32_t,uint32_t,uint32_t)>(adapter->hooks.original(StockHooks::SeatMods))(self,depressed,latched,locked,group);
}
void focus(CSeatManager* self, SP<CWLSurfaceResource> surface) {
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CSeatManager*,SP<CWLSurfaceResource>)>(adapter->hooks.original(StockHooks::Focus))(self,surface); return; }

#ifdef SLEDGE_STOCK_RESEARCH
    if (adapter->research) {
        adapter->research->syncUnlock(PROTO::sessionLock->isLocked());
        adapter->research->router.focusChanged();
        lifecycle(nullptr,7,0,2);
    }
#endif
    adapter->router.focusChanged(); // speculation only, never stock focus state
    reinterpret_cast<void (*)(CSeatManager*,SP<CWLSurfaceResource>)>(adapter->hooks.original(StockHooks::Focus))(self,surface);
}
void imeKey(CInputMethodKeyboardGrabV2* self, uint32_t time, uint32_t key, wl_keyboard_key_state state) {
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CInputMethodKeyboardGrabV2*,uint32_t,uint32_t,wl_keyboard_key_state)>(adapter->hooks.original(StockHooks::ImeKey))(self,time,key,state); return; }

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
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CInputMethodKeyboardGrabV2*,uint32_t,uint32_t,uint32_t,uint32_t)>(adapter->hooks.original(StockHooks::ImeMods))(self,depressed,latched,locked,group); return; }

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
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CSessionLockProtocol*,CExtSessionLockManagerV1*,uint32_t)>(adapter->hooks.original(StockHooks::LockEntry))(self,manager,id); return; }

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
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CWLSeatProtocol*,uint32_t)>(adapter->research->hooks.original(3))(self,caps); return; }

    reinterpret_cast<void (*)(CWLSeatProtocol*,uint32_t)>(adapter->research->hooks.original(3))(self,caps);
    adapter->replacement.observedCapabilities=caps;
    adapter->replacement.capabilitiesKnown=true;
}
void enter(CWLKeyboardResource* self, SP<CWLSurfaceResource> surface, wl_array* original) {
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CWLKeyboardResource*,SP<CWLSurfaceResource>,wl_array*)>(adapter->research->hooks.original(0))(self,surface,original); return; }

    adapter->replacement.entered(self,surface);
    if (original && original->size%sizeof(uint32_t)==0 && original->data) {
        const auto* keys=static_cast<const uint32_t*>(original->data);
        for (std::size_t i=0;i<original->size/sizeof(uint32_t);++i) adapter->stockLedger.seatRoute(keys[i],true);
    }
    auto* research=adapter->research.get();
    // Stack-backed wl_array, no wl_array_add allocation. Stock sendEnter copies
    // synchronously. Only committed forwarded union is visible in any new focus.
    if (adapter->startupOutputPending && !adapter->startupReplay) {
        // Before admission preserve stock's actual native keys on real focus
        // transitions. No consumed ownership exists in this startup state.
        reinterpret_cast<void (*)(CWLKeyboardResource*,SP<CWLSurfaceResource>,wl_array*)>(research->hooks.original(0))(self,surface,original);
        return;
    }
    std::array<uint32_t,keyCount> keys{}; unsigned count=0;
    for (uint32_t key=0; key<keyCount; ++key)
        if (research->forwarded.focusEnterKeys()[key]) keys[count++]=key;
    wl_array filtered{count*sizeof(uint32_t),keys.size()*sizeof(uint32_t),keys.data()};
    (void)original;
    reinterpret_cast<void (*)(CWLKeyboardResource*,SP<CWLSurfaceResource>,wl_array*)>(research->hooks.original(0))(self,surface,&filtered);
}
void resourceMods(CWLKeyboardResource* self, uint32_t depressed, uint32_t latched, uint32_t locked, uint32_t group) {
    if (adapter->drain.detached()) { reinterpret_cast<void (*)(CWLKeyboardResource*,uint32_t,uint32_t,uint32_t,uint32_t)>(adapter->research->hooks.original(1))(self,depressed,latched,locked,group); return; }

    if (adapter->startupOutputPending && !adapter->startupReplay) {
        reinterpret_cast<void (*)(CWLKeyboardResource*,uint32_t,uint32_t,uint32_t,uint32_t)>(adapter->research->hooks.original(1))(self,depressed,latched,locked,group);
        return;
    }
    if (adapter->research->revoking) {
        reinterpret_cast<void (*)(CWLKeyboardResource*,uint32_t,uint32_t,uint32_t,uint32_t)>(adapter->research->hooks.original(1))(self,0,0,0,0);
        return;
    }
    const auto& state=adapter->research->forwarded;
    reinterpret_cast<void (*)(CWLKeyboardResource*,uint32_t,uint32_t,uint32_t,uint32_t)>(adapter->research->hooks.original(1))
        (self,state.depressed(),state.latched(),state.locked(),state.group());
}
bool keybind(CKeybindManager* self, std::any event, SP<IKeyboard> keyboard) {
    if (adapter->drain.detached()) return reinterpret_cast<bool (*)(CKeybindManager*,std::any,SP<IKeyboard>)>(adapter->research->hooks.original(2))(self,std::move(event),keyboard);

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
APICALL EXPORT const char* SLEDGE_STOCK_ARTIFACT() {
#ifdef SLEDGE_STOCK_PRODUCTION
    return "sledge-stock-v1:" SLEDGE_STOCK_BUILD_ID;
#else
    return "stock-0.52.2:8dee6b387615f14e8ce6c1f7bbd0b58524c31d783061093423e6a3fc6f89b9ca";
#endif
}
APICALL EXPORT unsigned SLEDGE_STOCK_REGISTRATION_VERSION() { return registrationVersion; }
APICALL EXPORT unsigned SLEDGE_STOCK_WIRE_VERSION() { return wireVersion; }
APICALL EXPORT std::string PLUGIN_API_VERSION() { return HYPRLAND_API_VERSION; }
APICALL EXPORT void PLUGIN_EXIT() {
    if (!adapter) return;
    if (adapter->registryTimer) { adapter->registryTimer->cancel();g_pEventLoopManager->removeTimer(adapter->registryTimer);adapter->registryTimer.reset(); }
    if (adapter->statusCommand) {
        (void)HyprlandAPI::unregisterHyprCtlCommand(adapter->ownerHandle,adapter->statusCommand);
        adapter->statusCommand.reset();
    }
    if (adapter->drainDispatcherInstalled) {
        (void)HyprlandAPI::removeDispatcher(adapter->ownerHandle,"sledge:drain");
        adapter->drainDispatcherInstalled=false;
    }
    if (adapter->worker) {
        constexpr std::array<const char*,static_cast<unsigned>(StockWorker::Reason::Count)> labels{
            "connect_failed","hello_send_failed","hello_receive_timeout","hello_receive_malformed",
            "hello_late","hello_invalid","authenticated","requested_close","stopped",
            "idle_hangup","idle_unsolicited","idle_error","notify_failed","stale_event",
            "decision_failed","exception"};
        for (unsigned i=0;i<labels.size();++i)
            Debug::log(LOG,"SLEDGE_WORKER reason={} count={}",labels[i],
                adapter->worker->reasonCount(static_cast<StockWorker::Reason>(i)));
    }
#ifdef SLEDGE_STOCK_RESEARCH
    if (adapter->research) {
        constexpr std::array<const char*,static_cast<unsigned>(Adapter::CloseSource::Count)> closeLabels{
            "new_keyboard","destroy_keyboard","layout","consumer_changed","outcome_queue","unknown_input"};
        for (unsigned i=0;i<closeLabels.size();++i)
            Debug::log(LOG,"SLEDGE_RESEARCH_CLOSE source={} count={}",closeLabels[i],adapter->closeSources[i]);
        Debug::log(LOG,"SLEDGE_RESEARCH_CLOSE compatible_lifecycle={}",adapter->compatibleLifecycle);
        const auto& research=*adapter->research;
        const auto ownership=research.router.diagnostics();
        Debug::log(LOG,"SLEDGE_ROUTER owners={} forwarded={} consumed={} quarantined={} stock_consumed={} locker={} first_device={} first_key={} fatal={} pending={} locked={} stock_empty={}",
            ownership.owners,ownership.forwarded,ownership.consumed,ownership.quarantined,ownership.stockConsumed,
            ownership.locker,ownership.firstDevice,ownership.firstKey,ownership.fatal,ownership.pending,ownership.locked,adapter->stockLedger.empty());
        Debug::log(LOG,"SLEDGE_RESEARCH_EXIT registered={} modeled={} unmapped={} keymap_changes={} correlated_admissions={}",research.registeredCount,research.modeledEvents,research.unmappedEvents,research.keymapChanges,research.correlatedAdmissions);
        constexpr std::array<const char*,static_cast<unsigned>(AdmissionBlock::Count)> labels{
            "ready","maps_incompatible","no_consumer","consumer_unregistered","router_busy",
            "locked","nested","revoking","device_changed","physical_hold","unaccounted"};
        for (unsigned i=0;i<labels.size();++i)
            Debug::log(LOG,"SLEDGE_RESEARCH_BOUNDARY predicate={} count={}",labels[i],research.admissionChecks[i]);
        Debug::log(LOG,"SLEDGE_RESEARCH_BOUNDARY queued={} queue_failed={} not_ready={} map_deferred={} map_failed={} map_succeeded={}",
            research.boundaryQueued,research.boundaryQueueFailed,research.boundaryNotReady,
            research.mapRefreshDeferred,research.mapRefreshFailed,research.mapRefreshSucceeded);
        adapter->research->hooks.reset();
    }
#endif
    adapter->hooks.reset(); // verify byte restoration before worker join/unmap
    adapter.reset();
}
APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT(HANDLE handle) {
#ifdef SLEDGE_STOCK_CANDIDATE
    verifyCandidate(__hyprland_api_get_client_hash());
    const auto* candidateEndpoint=std::getenv("SLEDGE_HYPRLAND_PASSIVE_SOCKET");
    if (!candidateEndpoint || !*candidateEndpoint) throw std::runtime_error("candidate requires explicit daemon endpoint");
#endif
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
        // Production route is compiled but remains closed until immutable
        // registration verification is integrated; research env cannot open it.
#ifndef SLEDGE_STOCK_PRODUCTION
        if (const auto* mode=std::getenv("SLEDGE_STOCK_RESEARCH_MODE")) {
            const auto selected=std::string_view(mode);
            if (selected!="lock-quarantine" && selected!="replacement-wire") throw std::runtime_error("unknown isolated research mode");
            adapter->replacementWire=selected=="replacement-wire";
            if (const auto* drop=std::getenv("SLEDGE_STOCK_RESEARCH_DROP_OUTCOME_ONCE")) {
                if (std::string_view(drop)!="1") throw std::runtime_error("invalid drop Outcome fixture");
                adapter->dropOutcomeOnce=true;
            }
            adapter->research=std::make_unique<ResearchRoute>(adapter->hooks);
            adapter->research->lifecycle=[](void* context,unsigned opcode,uint64_t device,unsigned reason) {
                (void)lifecycle(context,opcode,device,reason);
            };
            adapter->research->registerExisting();
            installDrainControls(handle);
            installRegistryRetry();
            auto activeKeyboard=g_pSeatManager->m_keyboard.lock();
            if (!activeKeyboard || !adapter->replacement.setKeymap(activeKeyboard->m_xkbKeymap)) throw std::runtime_error("replacement keymap allocation failed");
            if (const auto* failure=std::getenv("SLEDGE_STOCK_RESEARCH_FAIL_AFTER")) {
                if (std::string_view(failure)=="preflight") adapter->replacement.rejectBefore=true;
                else if (std::string_view(failure)=="mods") adapter->replacement.failAfter=1;
                else if (std::string_view(failure)=="down") adapter->replacement.failAfter=2;
                else throw std::runtime_error("unknown replacement failure fixture");
            }
            adapter->research->hooks.install(handle,{reinterpret_cast<void*>(&enter),reinterpret_cast<void*>(&resourceMods),reinterpret_cast<void*>(&keybind),reinterpret_cast<void*>(&capabilities),reinterpret_cast<void*>(&newKeyboard),reinterpret_cast<void*>(&destroyKeyboard),reinterpret_cast<void*>(&setKeyboardLayout),reinterpret_cast<void*>(&keyboardResourceDestroyed),reinterpret_cast<void*>(&selectKeyboard)});
        }
#else
#ifdef SLEDGE_STOCK_CANDIDATE
        adapter->replacementWire=true;
#endif
        adapter->research=std::make_unique<ResearchRoute>(adapter->hooks);
        adapter->research->lifecycle=[](void* context,unsigned opcode,uint64_t device,unsigned reason) {
            (void)lifecycle(context,opcode,device,reason);
        };
        adapter->research->registerExisting(false);
        adapter->research->syncUnlock(PROTO::sessionLock->isLocked());
        installDrainControls(handle);
            installRegistryRetry();
        auto activeKeyboard=g_pSeatManager->m_keyboard.lock();
        if (!activeKeyboard || !adapter->replacement.setKeymap(activeKeyboard->m_xkbKeymap))
            throw std::runtime_error("production keymap allocation failed");
        adapter->research->hooks.install(handle,{reinterpret_cast<void*>(&enter),reinterpret_cast<void*>(&resourceMods),reinterpret_cast<void*>(&keybind),reinterpret_cast<void*>(&capabilities),reinterpret_cast<void*>(&newKeyboard),reinterpret_cast<void*>(&destroyKeyboard),reinterpret_cast<void*>(&setKeyboardLayout),reinterpret_cast<void*>(&keyboardResourceDestroyed),reinterpret_cast<void*>(&selectKeyboard)});
#endif // !SLEDGE_STOCK_PRODUCTION
        if (adapter->research) {
            refreshStockOutput([] { g_pInputManager->updateCapabilities(); },
                [] { reconcileStartupOutput(); });
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
            const std::string_view artifact=SLEDGE_STOCK_ARTIFACT();
            if (!wire::hello(hello,nonce,__hyprland_api_get_hash(),artifact)) throw std::runtime_error("passive Hello identity too long");
#ifdef SLEDGE_STOCK_RESEARCH
            adapter->worker = std::make_unique<StockWorker>(adapter->mailbox,endpoint,hello);
#else
            adapter->worker = std::make_unique<StockWorker>(adapter->mailbox,endpoint,hello);
#endif
        }
    } catch (...) { PLUGIN_EXIT(); throw; }
    return {"sledge-stock-passive", "Stock ABI lifecycle/worker; capabilities zero, no capture", "sledge", "1"};
}
