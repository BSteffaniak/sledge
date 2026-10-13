#pragma once
#include "decision_mailbox.hpp"
#include <limits>
#include <string>
namespace sledge::hyprland {
inline constexpr uint64_t ticketBudgetNs=10'000'000,commitReserveNs=1'000'000;
struct DurationMetrics {
    static constexpr std::array<uint64_t,8> upperNs{100'000,500'000,1'000'000,2'000'000,5'000'000,9'000'000,10'000'000,20'000'000};
    uint64_t count=0,maxNs=0,expired=0,clockErrors=0;
    std::array<uint64_t,9> buckets{};
    static void increment(uint64_t& value) noexcept { if (value!=std::numeric_limits<uint64_t>::max()) ++value; }
    void record(uint64_t start,uint64_t end,uint64_t deadline=0) noexcept {
        if (!start || !end || end<start) { increment(clockErrors);return; }
        const auto elapsed=end-start;
        increment(count);if (elapsed>maxNs) maxNs=elapsed;
        unsigned bucket=0;while (bucket<upperNs.size() && elapsed>upperNs[bucket]) ++bucket;
        increment(buckets[bucket]);
        if (deadline && end>=deadline) increment(expired);
    }
    std::string json() const {
        std::string result="{\"count\":"+std::to_string(count)+",\"max_ns\":"+std::to_string(maxNs)+
            ",\"expired\":"+std::to_string(expired)+",\"clock_errors\":"+std::to_string(clockErrors)+",\"buckets\":[";
        for (unsigned i=0;i<buckets.size();++i) { if (i) result+=",";result+=std::to_string(buckets[i]); }
        return result+"]}";
    }
};
class ScopedDuration {
    DurationMetrics& metrics;
    uint64_t start=monotonicNs(),deadline;
public:
    explicit ScopedDuration(DurationMetrics& target,uint64_t expires=0) noexcept : metrics(target),deadline(expires) {}
    ~ScopedDuration() { metrics.record(start,monotonicNs(),deadline); }
    ScopedDuration(const ScopedDuration&)=delete;
    ScopedDuration& operator=(const ScopedDuration&)=delete;
};
}
