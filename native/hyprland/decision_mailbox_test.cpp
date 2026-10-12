#include "decision_mailbox.hpp"
#include <cassert>
#include <thread>
#include <atomic>

using namespace sledge::hyprland;
int main() {
    DecisionMailbox mailbox;
    WirePacket event{}, answer{}, result{};
    event[0] = 3; answer[0] = 4;
    assert(!mailbox.decide(event,0,result));
    assert(!mailbox.decide(event,monotonicNs()+11'000'000,result));
    std::atomic<bool> taken = false;
    std::atomic<bool> finish = false;
    std::thread worker([&] {
        WirePacket received{}; uint64_t deadline = 0;
        while (!mailbox.take(received,deadline)) {
            if (finish.load()) return;
            std::this_thread::yield();
        }
        assert(received == event && deadline != 0);
        taken.store(true);
        while (!finish.load()) std::this_thread::yield();
        mailbox.complete(answer);
    });
    assert(!mailbox.decide(event,monotonicNs()+5'000'000,result));
    if (!taken.load()) {
        finish.store(true); worker.join();
        assert(false && "worker was not scheduled within callback deadline");
    }
    // Still in flight: timeout must not permit the next event to occupy slot.
    assert(!mailbox.decide(event,monotonicNs()+5'000'000,result));
    finish.store(true); worker.join();
    assert(result[0] == 0); // late reply was not delivered
    worker = std::thread([&] {
        WirePacket received{}; uint64_t deadline = 0;
        while (!mailbox.take(received,deadline)) std::this_thread::yield();
        mailbox.complete(answer);
    });
    assert(mailbox.decide(event,monotonicNs()+10'000'000,result));
    worker.join();
    assert(result == answer);
    // Unclaimed timeout is acknowledged by take, no phantom event returned.
    assert(!mailbox.decide(event,monotonicNs()+100'000,result));
    uint64_t deadline = 0;
    assert(!mailbox.take(result,deadline));
}
