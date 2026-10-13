#include "output_reconciliation.hpp"
#include "decision_mailbox.hpp"
#include <cassert>
#include <thread>
#include <atomic>
#include <csignal>
#include <pthread.h>
#include <chrono>

using namespace sledge::hyprland;
int main() {
    assert(outputReconciled(true,true,false,false,false));
    assert(!outputReconciled(false,true,false,false,false));
    assert(!outputReconciled(true,true,false,false,true));
    assert(!outputReconciled(true,true,true,false,false));
    assert(!outputReconciled(true,true,false,true,false));
    assert(!outputReconciled(true,false,false,false,false));
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
    timespec cpuStart{},cpuEnd{};
    assert(clock_gettime(CLOCK_THREAD_CPUTIME_ID,&cpuStart)==0);
    const auto started=monotonicNs();
    assert(!mailbox.decide(event,started+5'000'000,result));
    assert(monotonicNs()-started<50'000'000);
    assert(clock_gettime(CLOCK_THREAD_CPUTIME_ID,&cpuEnd)==0);
    const auto cpuNs=(cpuEnd.tv_sec-cpuStart.tv_sec)*1'000'000'000+(cpuEnd.tv_nsec-cpuStart.tv_nsec);
    assert(cpuNs<3'000'000); // 5ms deadline must sleep, not burn 5ms CPU
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
    // EINTR is a spurious wake: preserve the original absolute budget.
    struct sigaction action{};action.sa_handler=+[](int) {};sigemptyset(&action.sa_mask);
    assert(sigaction(SIGUSR1,&action,nullptr)==0);
    const auto caller=pthread_self();
    worker=std::thread([&] {
        WirePacket received{};uint64_t expires=0;
        while (!mailbox.take(received,expires)) std::this_thread::yield();
        for (unsigned i=0;i<3;++i) {
            assert(pthread_kill(caller,SIGUSR1)==0);
            std::this_thread::sleep_for(std::chrono::microseconds(200));
        }
        mailbox.complete(answer);
    });
    assert(mailbox.decide(event,monotonicNs()+10'000'000,result));worker.join();
    assert(result==answer);
    // Completion-before-wait/lost wake regression.
    for (unsigned i=0;i<100;++i) {
        worker=std::thread([&] {
            WirePacket received{};uint64_t expires=0;
            while (!mailbox.take(received,expires)) std::this_thread::yield();
            mailbox.complete(answer);
        });
        assert(mailbox.decide(event,monotonicNs()+10'000'000,result));
        worker.join();assert(result==answer);
    }
}
