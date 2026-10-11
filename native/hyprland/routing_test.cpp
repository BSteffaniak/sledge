#include "isolated_output.hpp"

#include <iostream>
#include <stdexcept>
#include <vector>

using namespace sledge::hyprland;
namespace {
void check(bool condition) { if (!condition) throw std::runtime_error("routing assertion failed"); }
void check(RouteResult result) { check(result.deliverClient); }
bool operator!(RouteResult result) { return !result.deliverClient; }
bool operator==(RouteResult result, bool expected) { return result.deliverClient == expected; }
struct RecordingOutput : Output {
    Delivery result = Delivery::Accepted;
    unsigned replacements = 0;
    uint8_t requested = 0, restored = 0;
    std::vector<uint32_t> releases;
    Delivery replace(uint32_t, uint8_t mods, uint8_t restore) override {
        ++replacements; requested = mods; restored = restore; return result;
    }
    bool release(uint32_t key) override { releases.push_back(key); return true; }
    bool reconcileModifiers(uint8_t sides) override { restored = sides; return true; }
};
Decision answer(const Ticket& ticket, Verdict verdict) { return {ticket, verdict, 31, 4}; }
void ownership() {
    Router router; RecordingOutput output;
    check(router.add(1)); check(router.add(2)); check(!router.add(1));
    router.unlockConfirmed();
    Event down{1, 30, State::Down};
    auto ticket = router.begin(down, 100, 10'000'000); check(ticket.has_value());
    check(!router.route(down, ticket, answer(*ticket, Verdict::Swallow), 101, output));
    router.focusChanged();
    check(!router.route({1,30,State::Repeat}, {}, {}, 102, output));
    check(router.route({2,30,State::Down}, {}, {}, 102, output));
    check(!router.route({1,30,State::Up}, {}, {}, 103, output));
    check(router.route({2,30,State::Up}, {}, {}, 104, output));
    check(router.route({1,900,State::Down}, {}, {}, 105, output));
    check(!router.begin({1, 200, State::Down, false}, 105, 100));
    check(router.route({1,200,State::Down,false}, {}, {}, 105, output));
    check(router.route({99,30,State::Down}, {}, {}, 105, output));
}
void replies() {
    Router router; RecordingOutput output; check(router.add(1)); router.unlockConfirmed();
    Event down{1,30,State::Down};
    auto ticket = router.begin(down, 100, 10); check(ticket.has_value());
    check(router.route(down, ticket, answer(*ticket, Verdict::Swallow), 110, output));
    check(router.route({1,30,State::Up}, {}, {}, 111, output));
    ticket = router.begin(down, 120, 10); router.focusChanged();
    check(router.route(down, ticket, answer(*ticket, Verdict::Swallow), 121, output));
    check(router.route({1,30,State::Up}, {}, {}, 122, output));
    ticket = router.begin(down, 130, 10); auto wrong = answer(*ticket,Verdict::Swallow); ++wrong.ticket.sequence;
    check(router.route(down, ticket, wrong, 131, output));
    check(router.route({1,30,State::Up}, {}, {}, 132, output));
    ticket = router.begin(down, 140, 10);
    auto forged = *ticket; forged.deadline = 1000;
    check(router.route(down, forged, answer(forged,Verdict::Swallow), 151, output));
    check(!router.begin({1,31,State::Down}, 0, 10'000'001));
}
void replacement() {
    for (const auto result : {Delivery::Accepted, Delivery::RejectedNoOutput, Delivery::PartialFatal}) {
        Router router; RecordingOutput output; output.result = result;
        check(router.add(1)); check(router.add(2)); router.unlockConfirmed();
        check(router.route({2,42,State::Down}, {}, {}, 1, output));
        Event down{1,30,State::Down}; auto ticket = router.begin(down, 10, 100);
        const bool pass = router.route(down, ticket, answer(*ticket,Verdict::Replace), 11, output).deliverClient;
        check(pass == (result == Delivery::RejectedNoOutput));
        check(output.replacements == 1 && output.requested == 4 && output.restored == 1);
        check(router.route({1,30,State::Up}, {}, {}, 12, output) == pass);
        if (result == Delivery::PartialFatal) check(!router.begin({1,31,State::Down},13,100));
    }
}
void lockAndRetirement() {
    Router router; RecordingOutput output; check(router.add(1)); check(router.add(2)); router.unlockConfirmed();
    check(router.route({1,42,State::Down}, {}, {}, 1, output));
    check(!router.route({2,42,State::Down}, {}, {}, 2, output));
    check(!router.route({1,42,State::Up}, {}, {}, 3, output));
    check(router.forwardedModifiers() == 1);
    auto ticket = router.begin({1,30,State::Down},4,100);
    router.lock(output);
    check(output.releases == std::vector<uint32_t>{42});
    check(router.forwardedModifiers() == 0);
    check(!router.begin({1,31,State::Down},5,100));
    check(router.route({1,30,State::Down}, ticket, answer(*ticket,Verdict::Replace), 6, output));
    check(output.replacements == 0);
    const auto lockerRelease = router.route({1,30,State::Up},{},{},7,output);
    check(lockerRelease.outcome == Outcome::LockerOwned);
    check(!router.route({2,42,State::Up}, {}, {}, 7, output));
    router.unlockConfirmed(); router.retire(1,output);
    check(!router.begin({1,31,State::Down},8,100));
    check(!router.add(1)); check(router.add(3));
}
void reviewedOutcomes() {
    Router router; RecordingOutput output; check(router.add(1)); check(router.add(2)); router.unlockConfirmed();
    // Preserve an unrelated consumed press across replacement precommit failure.
    auto ticket = router.begin({1,40,State::Down},1,100);
    check(!router.route({1,40,State::Down},ticket,answer(*ticket,Verdict::Swallow),2,output));
    check(router.route({2,31,State::Down}, {}, {}, 3, output));
    ticket = router.begin({1,30,State::Down},4,100);
    check(router.route({1,30,State::Down},ticket,answer(*ticket,Verdict::Replace),5,output));
    check(output.replacements == 0); // target held on other device: reject BEFORE writes.
    check(router.route({1,30,State::Repeat}, {}, {}, 6, output));
    check(router.route({1,30,State::Up}, {}, {}, 7, output));
    check(!router.route({1,40,State::Repeat}, {}, {}, 8, output));
    check(!router.route({1,40,State::Up}, {}, {}, 9, output));
    // A stock keybinding consumes the proposed modifier press: not delivered.
    check(router.route({1,42,State::Down}, {}, {}, 10, output));
    router.originalOutcome({1,42,State::Down},{Outcome::Forwarded,true,true,false},false);
    check(router.forwardedModifiers() == 0 && !router.focusEnterKeys()[42]);
    check(router.focusEnterKeys()[31]);
    router.focusChanged();
    check(router.focusEnterKeys()[31] && !router.focusEnterKeys()[42]);
    check(!router.route({1,42,State::Up}, {}, {}, 11, output));
    router.lock(output);
    check(!router.focusEnterKeys()[31]);
    check(output.releases == std::vector<uint32_t>{31});
    // Target collisions include consumed physical holds, not only seat state.
    router.unlockConfirmed();
    ticket = router.begin({1,31,State::Down},12,100);
    check(!router.route({1,31,State::Down},ticket,answer(*ticket,Verdict::Swallow),13,output));
    ticket = router.begin({1,30,State::Down},14,100);
    check(router.route({1,30,State::Down},ticket,answer(*ticket,Verdict::Replace),15,output));
    check(output.replacements == 0);
}
void fiveReviewRegressions() {
    Router router; RecordingOutput output; check(router.add(1)); check(router.add(2));
    auto locker = router.route({1,42,State::Down},{},{},1,output);
    check(locker.outcome == Outcome::LockerOwned && locker.processStock && locker.deliverClient);
    router.unlockConfirmed(); router.focusChanged(); router.disconnect(output);
    check(!router.focusEnterKeys()[42] && router.forwardedModifiers() == 0);
    auto repeat = router.route({1,42,State::Repeat},{},{},2,output);
    check(!repeat.processStock && !repeat.deliverClient);
    check(!router.route({1,42,State::Up},{},{},3,output));
    Event down{1,30,State::Down}; auto ticket = router.begin(down,4,100);
    Decision same{*ticket,Verdict::Replace,30,0};
    auto rejected = router.route(down,ticket,same,5,output);
    check(rejected.outcome == Outcome::Rejected && rejected.deliverClient && output.replacements == 0);
    check(router.route({1,30,State::Repeat},{},{},6,output));
    check(router.route({1,30,State::Up},{},{},7,output));
    auto proposed = router.route({1,40,State::Down},{},{},8,output);
    check(proposed.processStock);
    auto stockOutcome = router.originalOutcome({1,40,State::Down},proposed,false);
    check(stockOutcome.outcome == Outcome::StockConsumed && !stockOutcome.deliverClient);
    auto stockRepeat = router.route({1,40,State::Repeat},{},{},9,output);
    auto stockUp = router.route({1,40,State::Up},{},{},10,output);
    check(stockRepeat.outcome == Outcome::StockConsumed && stockRepeat.processStock && !stockRepeat.deliverClient);
    check(stockUp.outcome == Outcome::StockConsumed && stockUp.processStock && !stockUp.deliverClient);
    // A bridge must execute stock cleanup even though client delivery is denied.
    unsigned releaseBindings = 0, cancelledTimers = 0;
    if (stockUp.processStock) { ++releaseBindings; ++cancelledTimers; }
    check(releaseBindings == 1 && cancelledTimers == 1);
    ticket = router.begin({1,41,State::Down},11,100);
    check(router.route({1,41,State::Down},ticket,answer(*ticket,Verdict::Swallow),12,output).outcome == Outcome::Swallowed);
    check(router.route({2,42,State::Down},{},{},13,output));
    router.focusChanged(); router.disconnect(output);
    check(router.focusEnterKeys()[42] && !router.focusEnterKeys()[41]);
    check(!router.route({1,41,State::Up},{},{},14,output));
    check(router.route({2,42,State::Up},{},{},15,output));
    output.result = Delivery::PartialFatal;
    ticket = router.begin(down,16,100);
    auto partial = router.route(down,ticket,answer(*ticket,Verdict::Replace),17,output);
    check(partial.outcome == Outcome::PartialFatal && partial.reconcile && !partial.deliverClient && !partial.processStock);
    check(!router.begin({1,32,State::Down},18,100));
    check(!router.route({1,30,State::Up},{},{},19,output));
}
void mergedStockOutcome() {
    for (const bool admitted : {false,true}) {
        Router router; RecordingOutput output; check(router.add(1)); check(router.add(2)); router.unlockConfirmed();
        Event a{1,42,State::Down}, b{2,42,State::Down};
        auto first = router.route(a,{},{},1,output);
        check(router.originalOutcome(a,first,true).outcome == Outcome::Forwarded);
        auto merged = router.route(b,{},{},2,output);
        check(merged.outcome == Outcome::Merged && merged.processStock && !merged.deliverClient);
        auto outcome = router.originalOutcome(b,merged,admitted);
        check(outcome.outcome == (admitted ? Outcome::Merged : Outcome::StockConsumed));
        auto aUp = router.route({1,42,State::Up},{},{},3,output);
        check(aUp.deliverClient == !admitted);
        check(router.forwardedModifiers() == (admitted ? 1 : 0));
        check(router.focusEnterKeys()[42] == admitted);
        auto bUp = router.route({2,42,State::Up},{},{},4,output);
        check(bUp.processStock); // release binding/timer cleanup even if B rejected
        check(bUp.deliverClient == admitted);
        check(bUp.outcome == (admitted ? Outcome::Forwarded : Outcome::StockConsumed));
        check(router.forwardedModifiers() == 0 && !router.focusEnterKeys()[42]);
    }
}
void outputSequence() {
    struct Route : KeyRoute {
        bool ready = true;
        int failAt = -1;
        std::vector<int> writes;
        bool preflight(uint32_t, uint8_t) override { return ready; }
        bool modifiers(uint8_t value) override { writes.push_back(1000 + value); return static_cast<int>(writes.size()) != failAt; }
        bool key(uint32_t value, bool down) override { writes.push_back(static_cast<int>(value) + (down ? 2000 : 3000)); return static_cast<int>(writes.size()) != failAt; }
    };
    Route route; IsolatedOutput output(route);
    check(output.replace(30, 4, 1) == Delivery::Accepted);
    check(route.writes == std::vector<int>({1004,2030,3030,1001}));
    route.writes.clear(); route.ready = false;
    check(output.replace(30,4,1) == Delivery::RejectedNoOutput && route.writes.empty());
    route.ready = true;
    for (int failure = 1; failure <= 4; ++failure) {
        route.writes.clear(); route.failAt = failure;
        check(output.replace(30,4,1) == Delivery::PartialFatal);
        check(route.writes.back() == 1001); // best-effort restoration on every failure stage.
        if (failure == 2) check(route.writes == std::vector<int>({1004,2030,3030,1001}));
    }
}
} // namespace
int main() {
    ownership(); replies(); replacement(); lockAndRetirement(); reviewedOutcomes(); fiveReviewRegressions(); mergedStockOutcome(); outputSequence();
    std::cout << "PASS routing: ownership, stale/deadline replies, merged releases, modifier restore contract, lock quarantine, retirement\n";
}
