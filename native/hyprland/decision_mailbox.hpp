#pragma once

#include <array>
#include <atomic>
#include <cstdint>
#include <time.h>

namespace sledge::hyprland {
using WirePacket = std::array<uint8_t,512>;
inline uint64_t monotonicNs() noexcept {
    timespec now{};
    if (clock_gettime(CLOCK_MONOTONIC,&now) != 0) return 0;
    return static_cast<uint64_t>(now.tv_sec) * 1'000'000'000 + static_cast<uint64_t>(now.tv_nsec);
}
// One compositor producer / one socket worker. No pointers, heap, socket calls,
// locks or ownership mutations on the callback side. A timed-out in-flight slot
// cannot be reused until the worker acknowledges abandonment. Stop/join the
// worker before destroying the mailbox. Polling cadence is a worker concern.
class DecisionMailbox {
    enum class Phase : uint8_t { Idle, Request, Processing, Reply, Abandoned };
    static_assert(std::atomic<Phase>::is_always_lock_free);
    std::atomic<Phase> phase{Phase::Idle};
    WirePacket request{}, reply{};
    uint64_t deadline = 0;
public:
    DecisionMailbox() = default;
    DecisionMailbox(const DecisionMailbox&) = delete;
    DecisionMailbox& operator=(const DecisionMailbox&) = delete;
    bool decide(const WirePacket& event, uint64_t deadlineNs, WirePacket& decision) noexcept {
        const auto start = monotonicNs();
        if (!start || deadlineNs <= start || deadlineNs - start > 10'000'000 ||
            phase.load(std::memory_order_acquire) != Phase::Idle) return false;
        request = event;
        deadline = deadlineNs;
        phase.store(Phase::Request,std::memory_order_release);
        for (;;) {
            auto current = phase.load(std::memory_order_acquire);
            if (current == Phase::Reply) {
                const auto now = monotonicNs();
                const bool timely = now && now < deadlineNs;
                if (timely) decision = reply;
                phase.store(Phase::Idle,std::memory_order_release);
                return timely;
            }
            const auto now = monotonicNs();
            if (!now || now >= deadlineNs) {
                // Worker may publish concurrently. CAS avoids overwriting Reply;
                // loop then consumes it only if still before the deadline.
                if (phase.compare_exchange_strong(current,Phase::Abandoned,std::memory_order_acq_rel)) return false;
            }
        }
    }
    bool take(WirePacket& event, uint64_t& deadlineNs) noexcept {
        auto expected = Phase::Request;
        if (!phase.compare_exchange_strong(expected,Phase::Processing,std::memory_order_acq_rel)) {
            if (expected == Phase::Abandoned) phase.store(Phase::Idle,std::memory_order_release);
            return false;
        }
        event = request;
        deadlineNs = deadline;
        return true;
    }
    void complete(const WirePacket& decision) noexcept {
        reply = decision;
        auto expected = Phase::Processing;
        if (!phase.compare_exchange_strong(expected,Phase::Reply,std::memory_order_release,std::memory_order_relaxed))
            phase.store(Phase::Idle,std::memory_order_release);
    }
};
} // namespace sledge::hyprland
