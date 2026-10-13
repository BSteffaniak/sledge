#pragma once
#include <array>
namespace sledge::hyprland {
class DrainState {
    enum class Phase { Running, Draining, Detached };
    Phase phase=Phase::Running;
public:
    void request() noexcept { if (phase==Phase::Running) phase=Phase::Draining; }
    bool active() const noexcept { return phase!=Phase::Running; }
    bool detached() const noexcept { return phase==Phase::Detached; }
    bool mayPropose() const noexcept { return phase==Phase::Running; }
    bool ready(bool exactQuiescence) const noexcept { return active() && exactQuiescence; }
    bool commit(bool proof) noexcept {
        if (phase==Phase::Draining && proof) phase=Phase::Detached;
        return detached();
    }
};
class StockPressedLedger {
    std::array<bool,768> keys{};
    bool unknown=false;
public:
    void seatRoute(unsigned key,bool down) noexcept {
        if (key>=keys.size()) { unknown=true;return; }
        keys[key]=down;
    }
    bool empty() const noexcept {
        if (unknown) return false;
        for (bool held : keys) if (held) return false;
        return true;
    }
};
}
