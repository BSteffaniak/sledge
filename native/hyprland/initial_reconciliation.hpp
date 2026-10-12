#pragma once
#include "routing.hpp"
namespace sledge::hyprland {
// Stock has already updated physical state before InputManager key entry. Only
// the exact current down may be excluded from the pre-event hold proof; Router
// quiescence separately proves it is fresh (not an earlier committed hold).
// Compositor-local binding: only a fresh authenticated generation resets wire
// counters. Lifecycle controls may arrive before the first physical key.
inline bool bindConnection(uint64_t incomingGeneration,uint64_t incomingNonce,
                           uint64_t& generation,uint64_t& nonce,
                           uint64_t& sequence,uint64_t& epoch) noexcept {
    if (!incomingGeneration || !incomingNonce || incomingGeneration==generation) return false;
    generation=incomingGeneration;nonce=incomingNonce;sequence=0;epoch=0;
    return true;
}
enum class AdmissionBlock : unsigned {
    Ready, MapsIncompatible, NoConsumer, ConsumerUnregistered, RouterBusy,
    Locked, Nested, Revoking, DeviceChanged, PhysicalHold, Unaccounted, Count
};
// Lifecycle proof refresh may be deferred by holds without invalidating an
// unchanged previously proven map. Actual map mutation invalidates separately.
inline bool deferMapRefresh(bool ownershipEmpty,bool locked,bool nested,bool revoking) noexcept {
    return !ownershipEmpty || locked || nested || revoking;
}
inline bool preEventHold(bool pressed,const void* device,uint32_t key,
                         const void* entering,uint32_t enteringKey) noexcept {
    return pressed && !(device==entering && key==enteringKey);
}
} // namespace sledge::hyprland
