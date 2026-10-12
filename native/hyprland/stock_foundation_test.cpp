#include "hook_transaction.hpp"
#include "physical_devices.hpp"
#include "identity_policy.hpp"
#include "lock_phase.hpp"
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
    for (unsigned failure = 0; failure < 9; ++failure) {
        std::vector<unsigned> restored;
        std::array<Hook, 9> hook{{{0,true,false,restored},{1,true,false,restored},{2,true,false,restored},
            {3,true,false,restored},{4,true,false,restored},{5,true,false,restored},
            {6,true,false,restored},{7,true,false,restored},{8,true,false,restored}}};
        HookTransaction<Hook,9> transaction;
        for (unsigned i = 0; i < failure; ++i) assert(transaction.install(&hook[i]));
        hook[failure].succeeds = false;
        assert(!transaction.install(&hook[failure]));
        assert(transaction.size() == 0 && restored.size() == failure);
        for (unsigned i = 0; i < failure; ++i) assert(restored[i] == failure - i - 1);
        transaction.rollback();
        assert(restored.size() == failure);
    }
    std::vector<unsigned> restored;
    Hook first{1,true,false,restored};
    try {
        HookTransaction<Hook,9> transaction;
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
void recovery() {
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
    hooks(); devices(); phases(); recovery();
    for (const auto identity : {"", "unknown", "386376400119dd46a767c9f8c8791fd22c7b6e61"}) {
        bool rejected = false;
        try { requireQualifiedArtifact(identity); } catch (const std::runtime_error&) { rejected = true; }
        assert(rejected);
    }
}
