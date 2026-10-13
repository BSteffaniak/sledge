#pragma once
#include <array>
#include <cstdint>
namespace sledge::hyprland {
// Protocol retirement, not weak-pointer expiry or focus leave, discharges an
// affected output route. Addresses are compared only; never dereferenced.
class OutputRecovery {
    std::array<const void*,16> affected{};
    bool uncertain=false, overflow=false, freshBinding=false;
public:
    void begin() noexcept { uncertain=true;freshBinding=false; }
    void include(const void* resource) noexcept {
        if (!resource) { overflow=true;return; }
        for (auto& entry : affected) {
            if (entry==resource) return;
            if (!entry) { entry=resource;return; }
        }
        overflow=true;
    }
    void retired(const void* resource) noexcept {
        for (auto& entry : affected) if (entry==resource) entry=nullptr;
        freshBinding=false;
    }
    bool pending() const noexcept { return uncertain || overflow; }
    bool routesRetired() const noexcept {
        if (!uncertain || overflow) return false;
        for (const auto entry : affected) if (entry) return false;
        return true;
    }
    void entered() noexcept { if (routesRetired()) freshBinding=true; }
    bool eligible(bool physicalEmpty,bool qualified,bool unlocked) const noexcept {
        return routesRetired() && freshBinding && physicalEmpty && qualified && unlocked;
    }
    void completed() noexcept { uncertain=false;freshBinding=false; }
};
}
