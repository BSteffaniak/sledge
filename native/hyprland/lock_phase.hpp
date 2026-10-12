#pragma once
namespace sledge::hyprland {
// Early entry precedes stock m_locked=true. False during revocation/focus cannot
// prove unlock. Even observed true inside the original cannot allow nested false
// to reopen until that original has returned. No death/focus-based unlock.
class LockPhase {
    enum class Phase { Open, Entering, Locked };
    Phase phase = Phase::Open;
public:
    void enter() noexcept { phase=Phase::Entering; }
    bool routeLocked() const noexcept { return phase!=Phase::Open; }
    bool observe(bool protocolLocked) noexcept {
        if (phase==Phase::Entering) return false;
        if (protocolLocked) { phase=Phase::Locked; return false; }
        if (phase!=Phase::Locked) return false;
        phase=Phase::Open;
        return true;
    }
    void originalReturned(bool protocolLocked) noexcept {
        if (phase==Phase::Entering && protocolLocked) phase=Phase::Locked;
        // Failed/uncertain lock request stays closed; cannot claim unlock.
    }
};
} // namespace sledge::hyprland
