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
inline void invalidateFocusProof(Router& router,bool& reconciled) noexcept {
    reconciled=false;router.focusChanged(); // preserves committed ownership
}
inline bool bootstrapRouter(Router& router,bool& pending,bool fullReleasedProof,bool unlocked) {
    if (!pending) return true;
    if (!fullReleasedProof || !unlocked || !router.quiescent()) return false;
    router.unlockConfirmed();pending=false;return true;
}
template<class Bind,class Proof,class Emit>
void initialBoundary(bool& reconciled,Bind bind,Proof proof,Emit emit) {
    if (!bind() || reconciled) return;
    if (proof()) reconciled=emit();
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
// Retain a LIVE admission only across an identical stock consumer-map binding.
// Holds are deliberately not a criterion: this is not a new reason8 proof.
struct ConsumerSelectionProof {
    bool admitted=false,ready=false,sameMap=false,bound=false,stateConsistent=false;
    bool pending=false,locked=false,uncertain=false;
    bool retainsAdmission() const noexcept {
        return admitted && ready && sameMap && bound && stateConsistent &&
            !pending && !locked && !uncertain;
    }
};
inline bool preEventHold(bool pressed,const void* device,uint32_t key,
                         const void* entering,uint32_t enteringKey) noexcept {
    return pressed && !(device==entering && key==enteringKey);
}
} // namespace sledge::hyprland
