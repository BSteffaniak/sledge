#pragma once
namespace sledge::hyprland {
// Latched/locked bits are legitimate forwarded native XKB history, not holds.
// Synthetic modifier overrides must have restored that snapshot; any partial
// transaction is separately represented by pending recovery. Never clear Caps.
inline constexpr bool outputReconciled(bool stockEmpty,bool projectionKnown,
    bool projectedHolds,bool depressed,bool recoveryPending) noexcept {
    return stockEmpty && projectionKnown && !projectedHolds && !depressed && !recoveryPending;
}
}
