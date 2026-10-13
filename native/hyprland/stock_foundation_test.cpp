#include "output_reconciliation.hpp"
#include "timing_metrics.hpp"
#include "drain_state.hpp"
#include "enter_completion.hpp"
#include "startup_output.hpp"
#include "output_recovery.hpp"
#include "hook_bytes.hpp"
#include "hook_transaction.hpp"
#include "physical_devices.hpp"
#include "identity_policy.hpp"
#include "lock_phase.hpp"
#include "initial_reconciliation.hpp"
#include <cassert>
#include <vector>

using namespace sledge::hyprland;
namespace {
struct Hook {
    unsigned id;
    bool succeeds = true;
    bool active = false;
    std::vector<unsigned>& restored;
    bool hook() { active = succeeds; return succeeds; }
    bool unhook() { assert(active); active = false; restored.push_back(id); return true; }
};
struct Sink : Output {
    unsigned released = 0;
    Delivery replace(uint32_t, uint8_t, uint8_t) override { return Delivery::RejectedNoOutput; }
    bool release(uint32_t) override { ++released; return true; }
    bool reconcileModifiers(uint8_t) override { return true; }
};
void hooks() {
    for (unsigned failure = 0; failure < 10; ++failure) {
        std::vector<unsigned> restored;
        std::array<Hook, 10> hook{{{0,true,false,restored},{1,true,false,restored},{2,true,false,restored},
            {3,true,false,restored},{4,true,false,restored},{5,true,false,restored},
            {6,true,false,restored},{7,true,false,restored},{8,true,false,restored},{9,true,false,restored}}};
        HookTransaction<Hook,10> transaction;
        for (unsigned i = 0; i < failure; ++i) assert(transaction.install(&hook[i]));
        hook[failure].succeeds = false;
        assert(!transaction.install(&hook[failure]));
        assert(transaction.size() == 0 && restored.size() == failure);
        for (unsigned i = 0; i < failure; ++i) assert(restored[i] == failure - i - 1);
        for (const auto& entry : hook) assert(!entry.active);
        transaction.rollback();
        assert(restored.size() == failure);
    }
    // Full ten-hook success/rollback verifies every saved source snapshot.
    struct ByteHook {
        std::array<uint8_t,64> bytes{};
        bool hook() { bytes[0]=0xe9;return true; }
        bool unhook() { bytes[0]=0;return true; }
    };
    std::array<ByteHook,10> all{};
    std::array<HookBytes,10> snapshots{};
    HookTransaction<ByteHook,10> full;
    for (unsigned i=0;i<all.size();++i) {
        snapshots[i].snapshot(all[i].bytes.data());
        assert(full.install(&all[i]) && !snapshots[i].restored());
    }
    assert(full.size()==10);full.rollback();
    for (const auto& snapshot : snapshots) { assert(snapshot.restored());snapshot.requireRestored(); }
    std::vector<unsigned> restored;
    Hook first{1,true,false,restored};
    try {
        HookTransaction<Hook,10> transaction;
        assert(transaction.install(&first));
        throw 1;
    } catch (int) {}
    assert(restored == std::vector<unsigned>{1});
    HookTransaction<Hook,1> transaction;
    assert(transaction.install(&first));
    assert(!transaction.install(&first)); // capacity rejection rolls back too
    assert(!first.active);
}
void devices() {
    PhysicalDevices devices; Router router; Sink sink;
    int physical = 0, second = 0;
    std::array<uint32_t,keyCount> map{};
    map.fill(PhysicalDevices::unmapped);
    map[30] = 30; map[42] = 42;
    assert(!devices.attach(&physical,false,false,map,router));
    assert(!devices.attach(&physical,true,true,map,router));
    auto bad = map; bad[31] = 30;
    assert(!devices.attach(&physical,true,false,bad,router));
    const auto generation = devices.attach(&physical,true,false,map,router);
    assert(generation && !devices.attach(&physical,true,false,map,router));
    router.unlockConfirmed();
    assert(!devices.event(&second,30,true));
    assert(!devices.event(&physical,keyCount,true));
    assert(!devices.event(&physical,31,true));
    assert(!devices.event(&physical,30,false));
    auto down = devices.event(&physical,42,true);
    assert(down && down->device == generation && down->state == State::Down);
    assert(router.route(*down,{},{},1,sink).deliverClient);
    assert(devices.event(&physical,42,true)->state == State::Repeat);
    router.focusChanged(); router.disconnect(sink);
    assert(router.focusEnterKeys()[42]);
    router.lock(sink);
    assert(sink.released == 1 && !router.focusEnterKeys()[42]);
    router.unlockConfirmed();
    auto up = devices.event(&physical,42,false);
    assert(up && !router.route(*up,{},{},2,sink).deliverClient);
    assert(devices.detach(&physical,router,sink));
    assert(!devices.event(&physical,42,true));
    const auto replacement = devices.attach(&physical,true,false,map,router);
    assert(replacement > generation); // same address never reuses stream identity
    int power=0;
    auto mediaMap=map;mediaMap.fill(PhysicalDevices::unmapped);mediaMap[116]=116;
    const auto media=devices.attach(&power,true,false,mediaMap,router);
    assert(media>replacement); // non-typing physical keyboard participates in proof
    auto powerDown=devices.event(&power,116,true);
    assert(powerDown && router.route(*powerDown,{},{},3,sink).deliverClient);
    assert(!router.quiescent());
    auto powerUp=devices.event(&power,116,false);
    assert(powerUp);(void)router.route(*powerUp,{},{},4,sink);
    assert(router.quiescent());
}
void phases() {
    LockPhase phase; Router router; Sink sink;
    assert(router.add(1)); router.unlockConfirmed();
    for (unsigned cycle=0; cycle<2; ++cycle) {
        phase.enter(); router.lock(sink);
        // Focus/revocation callback before stock original sets true: cannot open
        // routing or allow the old IME. Even nested true/false stays Entering.
        assert(phase.routeLocked() && !phase.observe(false));
        assert(!phase.observe(true) && !phase.observe(false) && phase.routeLocked());
        phase.originalReturned(true);
        assert(phase.routeLocked() && !phase.observe(true));
        const auto press=router.route({1,54,State::Down},{},{},1,sink);
        assert(press.outcome==Outcome::LockerOwned && press.deliverClient);
        assert(router.lockerEnterKeys()[54] && !router.focusEnterKeys()[54]);
        assert(phase.observe(false)); router.unlockConfirmed();
        assert(!phase.routeLocked() && !phase.observe(false));
        assert(!router.focusEnterKeys()[54]);
        assert(!router.route({1,54,State::Repeat},{},{},2,sink).deliverClient);
        assert(!router.route({1,54,State::Up},{},{},3,sink).deliverClient);
    }
    phase.enter(); phase.originalReturned(false);
    assert(!phase.observe(false) && phase.routeLocked()); // rejected request stays uncertain
}
void pendingStartupDrain() {
    DrainState drain;StockPressedLedger ledger;OutputRecovery uncertain;
    Router router;Sink sink;assert(router.add(1));router.unlockConfirmed();
    const bool startupPending=true;
    const auto proof=[&](bool retiring) {
        return router.quiescent() && outputBoundaryReady(
            outputReconciled(ledger.empty(),true,false,false,uncertain.pending()),
            false,startupPending,retiring);
    };
    assert(!proof(false)); // waiting startup cannot admit Reset8
    drain.request();ledger.seatRoute(30,true);
    assert(!drain.commit(proof(true)) && !drain.detached());
    ledger.seatRoute(30,false);
    int resource=0;uncertain.begin();uncertain.include(&resource);
    assert(!drain.commit(proof(true))); // startup bypass never bypasses uncertainty
    uncertain.retired(&resource);
    assert(!drain.commit(proof(true))); // destroyed route alone insufficient
    uncertain.entered();assert(uncertain.eligible(true,true,true));uncertain.completed();
    assert(!proof(false) && proof(true));
    assert(drain.commit(proof(true)) && drain.detached());
    assert(!drain.mayPropose() && drain.commit(false));
}
void idleAdmission() {
    bool reconciled=false,connected=false,naturalEnter=false;
    uint64_t generation=0,nonce=0,sequence=0,epoch=0;
    std::vector<unsigned> stream;
    const auto tick=[&] {
        initialBoundary(reconciled,[&] {
            if (!connected) return false;
            (void)bindConnection(1,123,generation,nonce,sequence,epoch);
            return true;
        },[&] { return naturalEnter; },[&] {
            assert(nonce==123 && generation==1);stream.push_back(8);return true;
        });
    };
    naturalEnter=true;tick();assert(!reconciled && stream.empty());
    connected=true;tick();assert(reconciled && stream==std::vector<unsigned>{8});
    tick();assert(stream.size()==1); // no repeated idle Reset8
    // First fresh Down after idle proof and verified consumer selection creates
    // a new proof before a pending ticket; no sacrificial physical event.
    Router router;assert(router.add(1));router.unlockConfirmed();
    router.focusChanged();reconciled=false;
    initialBoundary(reconciled,[] { return true; },[&] { return router.quiescent(); },[] { return true; });
    assert(reconciled);
    const auto ticket=router.begin({1,30,State::Down},1,10'000'000);
    assert(ticket && !router.quiescent());
    struct Replacement : Output {
        Delivery replace(uint32_t,uint8_t,uint8_t) override { return Delivery::Accepted; }
        bool release(uint32_t) override { return true; }
        bool reconcileModifiers(uint8_t) override { return true; }
    } output;
    const Decision answer{*ticket,Verdict::Replace,48,0};
    assert(router.route({1,30,State::Down},ticket,answer,2,output).outcome==Outcome::Replaced);
    stream.push_back(3);assert(stream[0]==8 && stream[1]==3); // first Event later
}
void productionBootstrap() {
    Router router;PhysicalDevices devices;Sink sink;
    int keyboard=0;std::array<uint32_t,keyCount> map{};
    map.fill(PhysicalDevices::unmapped);map[30]=30;
    assert(devices.attach(&keyboard,true,false,map,router));
    bool bootstrap=true;
    assert(!bootstrapRouter(router,bootstrap,false,true));
    assert(!bootstrapRouter(router,bootstrap,true,false));
    // Production starts registered but Router locked; no fabricated friendly
    // Event/unlock. Model the exact physical Down before begin like stock.
    const auto down=devices.event(&keyboard,30,true);assert(down && down->state==State::Down);
    assert(!router.begin(*down,1,10'000'000));
    assert(router.lastBeginReject==Router::BeginReject::Locked);
    const auto up=devices.event(&keyboard,30,false);assert(up);
    assert(bootstrapRouter(router,bootstrap,true,true) && !bootstrap);
    const auto fresh=devices.event(&keyboard,30,true);assert(fresh && fresh->state==State::Down);
    const auto ticket=router.begin(*fresh,2,10'000'000);assert(ticket);
    assert(router.lastBeginReject==Router::BeginReject::None);
    router.route(*fresh,ticket,Decision{*ticket,Verdict::Swallow,0,0},3,sink);
    const auto repeat=devices.event(&keyboard,30,true);assert(repeat && repeat->state==State::Repeat);
    assert(!router.begin(*repeat,4,10'000'000));
    router.lock(sink);
    assert(bootstrapRouter(router,bootstrap,true,true)); // no second bootstrap unlock
    assert(router.diagnostics().locked);
}
void focusAdmission() {
    Router router;Sink output;assert(router.add(1));router.unlockConfirmed();
    bool admitted=true,entered=false;
    const Event held{1,30,State::Down};
    const auto ticket=router.begin(held,1,10'000'000);assert(ticket);
    assert(router.route(held,ticket,Decision{*ticket,Verdict::Swallow,0,0},2,output).outcome==Outcome::Swallowed);
    const auto epoch=router.lockEpoch();
    invalidateFocusProof(router,admitted);
    assert(!admitted && router.lockEpoch()>epoch && !router.ownershipEmpty());
    const auto reconcile=[&] {
        initialBoundary(admitted,[] { return true; },[&] { return entered && router.quiescent(); },[] { return true; });
    };
    reconcile();assert(!admitted); // empty/no completed focus route
    EnterCompletion actualEnter;actualEnter.completed(true);entered=actualEnter.observed();
    reconcile();assert(!admitted); // real enter cannot waive consumed hold
    assert(!router.route({1,30,State::Repeat},{},{},3,output).deliverClient);
    assert(!router.route({1,30,State::Up},{},{},4,output).deliverClient);
    reconcile();assert(admitted && router.quiescent());
    assert(router.begin({1,30,State::Down},5,10'000'000));
}
void registryProofOrdering() {
    Router router;Sink output;assert(router.add(1));router.unlockConfirmed();
    bool admitted=true;
    std::vector<unsigned> stream;
    // Timer learns a newly attached, released physical keyboard after an
    // earlier proof: invalidate speculation, retain owners, arm Reset3 first.
    assert(router.add(2));
    admitted=false;router.focusChanged();stream.push_back(3);
    initialBoundary(admitted,[] { return true; },[&] { return router.quiescent(); },[&] {
        stream.push_back(8);return true;
    });
    assert(admitted && stream==std::vector<unsigned>({3,8}));
    const auto ticket=router.begin({2,30,State::Down},1,10'000'000);assert(ticket);
    router.route({2,30,State::Down},ticket,Decision{*ticket,Verdict::Swallow,0,0},2,output);
    const auto first=router.begin({1,42,State::Down},3,10'000'000);assert(first);
    router.route({1,42,State::Down},first,Decision{*first,Verdict::Pass,0,0},4,output);
    assert(router.forwardedModifiers()!=0 && !router.ownershipEmpty());
}
void realLockEmptyOutput() {
    struct Unconfirmed : Output {
        unsigned writes=0;
        Delivery replace(uint32_t,uint8_t,uint8_t) override { return Delivery::RejectedNoOutput; }
        bool release(uint32_t) override { ++writes;return false; }
        bool reconcileModifiers(uint8_t) override { ++writes;return false; }
    } output;
    Router router;LockPhase phase;assert(router.add(1));router.unlockConfirmed();
    phase.enter();router.lock(output);
    assert(output.writes==0 && !router.diagnostics().fatal);
    assert(!phase.observe(false));phase.originalReturned(true);
    const Event down{1,30,State::Down},up{1,30,State::Up};
    auto proposal=router.route(down,{},{},1,output);
    assert(proposal.outcome==Outcome::LockerOwned);
    router.originalOutcome(down,proposal,true);
    assert(router.route(up,{},{},2,output).deliverClient);
    assert(phase.observe(false));router.unlockConfirmed();
    assert(router.quiescent() && !router.diagnostics().locked);
    assert(router.begin(down,3,10'000'000));
}
void consumerSelection() {
    ConsumerSelectionProof healthy{true,true,true,true,true,false,false,false};
    assert(healthy.retainsAdmission());
    for (auto field : {&ConsumerSelectionProof::admitted,&ConsumerSelectionProof::ready,
                      &ConsumerSelectionProof::sameMap,&ConsumerSelectionProof::bound,
                      &ConsumerSelectionProof::stateConsistent}) {
        auto failed=healthy;failed.*field=false;assert(!failed.retainsAdmission());
    }
    for (auto field : {&ConsumerSelectionProof::pending,&ConsumerSelectionProof::locked,&ConsumerSelectionProof::uncertain}) {
        auto failed=healthy;failed.*field=true;assert(!failed.retainsAdmission());
    }
    Router router;Sink output;assert(router.add(1) && router.add(2));router.unlockConfirmed();
    Event alt{1,56,State::Down};
    const auto proposal=router.route(alt,{},{},1,output);
    router.originalOutcome(alt,proposal,true);
    assert(!router.quiescent() && router.forwardedModifiers()!=0);
    // Retained proof is not a reason8 assertion; USB Alt remains physically
    // owned while the second keyboard's A is synchronously replaced.
    assert(healthy.retainsAdmission());
    const auto ticket=router.begin({2,30,State::Down},2,10'000'000);assert(ticket);
    struct AcceptedSink : Sink { Delivery replace(uint32_t,uint8_t,uint8_t) override { return Delivery::Accepted; } } accepted;
    const auto replaced=router.route({2,30,State::Down},ticket,Decision{*ticket,Verdict::Replace,48,0},3,accepted);
    assert(replaced.outcome==Outcome::Replaced && router.forwardedModifiers()!=0);
    router.route({2,30,State::Up},{},{},4,output);
    assert(router.forwardedModifiers()!=0);
    const auto up=router.route({1,56,State::Up},{},{},5,output);
    router.originalOutcome({1,56,State::Up},up,true);
    assert(router.quiescent() && router.forwardedModifiers()==0);
}
void timingMetrics() {
    DurationMetrics metrics;
    for (unsigned i=0;i<DurationMetrics::upperNs.size();++i) {
        const auto limit=DurationMetrics::upperNs[i];
        metrics.record(1,1+limit);assert(metrics.buckets[i]>=1);
        metrics.record(1,2+limit);assert(metrics.buckets[i+1]>=1);
    }
    assert(metrics.count==16 && metrics.maxNs==20'000'001);
    metrics.record(100,200,200);assert(metrics.expired==1);
    metrics.record(0,200);metrics.record(200,100);assert(metrics.clockErrors==2);
    metrics.count=UINT64_MAX;metrics.record(1,2);assert(metrics.count==UINT64_MAX);
    DurationMetrics scoped;
    const auto early=[&] { ScopedDuration duration(scoped);return; };
    early();assert(scoped.count==1 || scoped.clockErrors==1);
    assert(ticketBudgetNs-commitReserveNs==9'000'000);
}
void recovery() {
    DrainState drain;
    assert(drain.mayPropose() && !drain.ready(true));
    drain.request();assert(!drain.mayPropose() && drain.active());
    assert(!drain.ready(false) && drain.ready(true));
    drain.request();assert(!drain.mayPropose());
    assert(!drain.ready(false)); // later input invalidates readiness, not drain intent
    assert(!drain.commit(false) && !drain.detached());
    assert(drain.commit(true) && drain.detached());
    drain.request();assert(drain.detached() && !drain.mayPropose());
    assert(drain.commit(false)); // durable after later physical input
    StockPressedLedger ledger;
    assert(ledger.empty());ledger.seatRoute(30,true);assert(!ledger.empty());
    ledger.seatRoute(30,false);assert(ledger.empty());
    ledger.seatRoute(768,true);assert(!ledger.empty());
    OutputRecovery invalid;
    invalid.begin();invalid.include(nullptr);invalid.entered();
    assert(invalid.pending() && !invalid.routesRetired() && !invalid.eligible(true,true,true));
    OutputRecovery proof;
    int oldA=0,oldB=0;
    proof.begin();proof.include(&oldA);proof.include(&oldB);
    proof.entered();assert(!proof.eligible(true,true,true));
    proof.retired(&oldA);proof.entered();assert(!proof.routesRetired());
    proof.retired(&oldB);assert(proof.routesRetired());
    assert(!proof.eligible(true,true,true)); // retirement requires later fresh binding
    proof.entered();
    assert(!proof.eligible(false,true,true));
    assert(!proof.eligible(true,false,true));
    assert(!proof.eligible(true,true,false));
    assert(proof.eligible(true,true,true));
    proof.completed();assert(!proof.eligible(true,true,true));
    uint64_t generation=0,nonce=0,sequence=0,epoch=0;
    assert(bindConnection(1,17,generation,nonce,sequence,epoch));
    // Focus emits four controls before any key callback.
    for (unsigned i=1;i<=4;++i) assert(++sequence==i);
    epoch=6;
    assert(!bindConnection(1,17,generation,nonce,sequence,epoch));
    assert(sequence==4 && epoch==6 && nonce==17);
    assert(++sequence==5); // first-key boundary never regresses
    assert(bindConnection(2,18,generation,nonce,sequence,epoch));
    assert(sequence==0 && epoch==0 && nonce==18 && generation==2);
    assert(++sequence==1);
    assert(!bindConnection(2,18,generation,nonce,sequence,epoch));
    assert(sequence==1);
    assert(deferMapRefresh(false,false,false,false));
    assert(deferMapRefresh(true,true,false,false));
    assert(deferMapRefresh(true,false,true,false));
    assert(deferMapRefresh(true,false,false,true));
    assert(!deferMapRefresh(true,false,false,false));
    int first=0,second=0;
    assert(!preEventHold(true,&first,30,&first,30));
    assert(preEventHold(true,&first,42,&first,30));
    assert(preEventHold(true,&second,30,&first,30));
    assert(!preEventHold(false,&second,30,&first,30));
    Router router; Sink sink; LockPhase phase;
    assert(router.add(1)); router.unlockConfirmed();
    // Observable stock duplicate-down ledger model, anchored to stock
    // InputManager::onKeyboardKey m_pressed admission/removal before seatKey.
    std::array<bool,keyCount> stockPressed{};
    unsigned clientDown=0, clientUp=0;
    auto stock=[&](Event event, RouteResult result) {
        if (stockPressed[event.key]==(event.state!=State::Up)) return;
        stockPressed[event.key]=event.state!=State::Up;
        if (result.deliverClient) {
            if (event.state==State::Up) ++clientUp; else ++clientDown;
        }
    };
    Event shift{1,42,State::Down};
    stock(shift,router.route(shift,{},{},1,sink));
    assert(stockPressed[42] && clientDown==1);
    phase.enter(); router.lock(sink); phase.originalReturned(true);
    const Event release{1,42,State::Up};
    auto quarantined=router.route(release,{},{},2,sink);
    assert(quarantined.outcome==Outcome::Quarantined && !quarantined.deliverClient);
    // R6 full InputManager cleanup executes even though processStock is false.
    stock(release,quarantined);
    assert(!stockPressed[42] && clientUp==0);
    assert(phase.observe(false)); router.unlockConfirmed();
    stock(shift,router.route(shift,{},{},3,sink));
    stock(release,router.route(release,{},{},4,sink));
    assert(clientDown==2 && clientUp==1 && !stockPressed[42]);
}
}
int main() {
    // Pre-existing focused resource: no natural post-load enter/caps event.
    // Admission stays closed until authoritative stock callbacks are replayed.
    bool caps=false,bound=false;unsigned order=0;
    assert(!(caps && bound));
    refreshStockOutput([&] { assert(order++==0);caps=true; },
        [&] { assert(order++==1 && caps);bound=true; });
    assert(order==2 && caps && bound);
    // A missing stock enter must not fabricate a binding.
    bound=false;
    refreshStockOutput([&] { caps=true; },[] {});
    assert(!bound);
    assert(startupReplayAllowed(true,true,true,false));
    assert(!startupReplayAllowed(false,true,true,false));
    assert(!startupReplayAllowed(true,false,true,false));
    assert(!startupReplayAllowed(true,true,false,false));
    assert(!startupReplayAllowed(true,true,true,true));
    // A held pre-existing client receives no replay/leave/release until drained.
    unsigned emitted=0;
    if (startupReplayAllowed(false,false,true,true)) ++emitted;
    assert(emitted==0);
    if (startupReplayAllowed(true,true,true,false)) ++emitted;
    assert(emitted==1);
    EnterCompletion noOp;
    assert(!noOp.observed()); // wrapper same-surface return gives no authority
    noOp.completed(false);assert(!noOp.observed());
    EnterCompletion actual;
    actual.completed(true);assert(actual.observed());
    hooks(); devices(); phases(); recovery(); pendingStartupDrain(); idleAdmission(); productionBootstrap(); focusAdmission(); realLockEmptyOutput(); registryProofOrdering(); timingMetrics(); consumerSelection();
    for (const auto identity : {"", "unknown", "386376400119dd46a767c9f8c8791fd22c7b6e61"}) {
        bool rejected = false;
        try { requireQualifiedArtifact(identity); } catch (const std::runtime_error&) { rejected = true; }
        assert(rejected);
    }
}
