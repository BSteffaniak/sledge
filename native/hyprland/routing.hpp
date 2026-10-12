#pragma once

#include <array>
#include <cstdint>
#include <optional>

namespace sledge::hyprland {

constexpr uint32_t keyCount = 768; // Linux KEY_MAX + 1; larger codes bypass unchanged.
constexpr unsigned deviceCount = 16;
enum class State { Up, Down, Repeat };
enum class Verdict { Pass, Swallow, Replace };
enum class Delivery { Accepted, RejectedNoOutput, PartialFatal };
enum class Outcome { Forwarded, Merged, Swallowed, Replaced, Rejected, PartialFatal, StockConsumed, LockerOwned, Quarantined };
struct RouteResult {
    Outcome outcome;
    bool processStock;
    bool deliverClient;
    bool reconcile;
};
struct Event { uint64_t device; uint32_t key; State state; bool modeled = true; };
struct Ticket {
    uint64_t device, epoch, focus, sequence, deadline;
    uint32_t key;
    bool operator==(const Ticket&) const = default;
};
struct Decision { Ticket ticket; Verdict verdict; uint32_t key = 0; uint8_t modifiers = 0; };

// Compositor-thread boundary. Implementations MUST preflight replacement routing
// and emit requested modifiers, down/up, restored modifiers without re-entry.
// Accepted means route acceptance, not application receipt. RejectedNoOutput
// guarantees ZERO modifier/key output; partial output is never fallback-pass.
struct Output {
    virtual ~Output() = default;
    virtual Delivery replace(uint32_t key, uint8_t requested, uint8_t restore) = 0;
    virtual bool release(uint32_t key) = 0;
    virtual bool reconcileModifiers(uint8_t sides) = 0;
};

class Router {
    enum class Owner : uint8_t { None, Forwarded, Consumed, Quarantined, StockConsumed, LockerOwned };
    struct Device {
        uint64_t id = 0;
        std::array<Owner, keyCount> owners{};
    };
    std::array<Device, deviceCount> devices{};
    uint64_t epoch = 1, focus = 1, sequence = 0, lastGeneration = 0;
    bool locked = true, fatal = false;
    std::optional<Ticket> pending;

    Device* find(uint64_t id) {
        for (auto& device : devices)
            if (device.id == id && id != 0) return &device;
        return nullptr;
    }
    static uint8_t modifier(uint32_t key) {
        constexpr std::array<uint32_t, 8> keys{42, 54, 29, 97, 56, 100, 125, 126};
        for (unsigned i = 0; i < keys.size(); ++i)
            if (key == keys[i]) return static_cast<uint8_t>(1U << i);
        return 0;
    }
    bool otherForwarded(uint64_t id, uint32_t key) const {
        for (const auto& device : devices)
            if (device.id != id && device.id != 0 && device.owners[key] == Owner::Forwarded) return true;
        return false;
    }
public:
    bool add(uint64_t id) {
        if (id == 0 || id <= lastGeneration || find(id)) return false;
        for (auto& device : devices)
            if (device.id == 0) { device.id = id; lastGeneration = id; return true; }
        return false; // caller must bypass untracked devices, not share identity.
    }
    uint8_t forwardedModifiers() const {
        uint8_t result = 0;
        for (const auto& device : devices)
            if (device.id != 0)
                for (uint32_t key = 0; key < keyCount; ++key)
                    if (device.owners[key] == Owner::Forwarded) result |= modifier(key);
        return result;
    }
    std::optional<Ticket> begin(Event event, uint64_t now, uint64_t budgetNs) {
        auto* device = find(event.device);
        if (!device || pending || sequence == UINT64_MAX || locked || fatal || !event.modeled || event.key >= keyCount || event.state != State::Down ||
            device->owners[event.key] != Owner::None || budgetNs > 10'000'000 ||
            now > UINT64_MAX - budgetNs) return std::nullopt;
        pending = Ticket{event.device, epoch, focus, ++sequence, now + budgetNs, event.key};
        return pending;
    }
    // Correlation only for committed lifecycle settlement. Does not create a
    // matcher proposal or pending routing authority; valid even after fatal
    // replacement uncertainty so physical releases can retire exact ownership.
    std::optional<Ticket> settlement(Event event,uint64_t now,uint64_t budgetNs) {
        if (!find(event.device) || locked || !event.modeled || event.key>=keyCount ||
            event.state==State::Down || sequence==UINT64_MAX || budgetNs>10'000'000 ||
            now>UINT64_MAX-budgetNs) return {};
        return Ticket{event.device,epoch,focus,++sequence,now+budgetNs,event.key};
    }
    // Return true to pass to the stock route. Existing ownership always wins,
    // including repeats/releases after timeout, focus changes and disconnection.
    RouteResult route(Event event, const std::optional<Ticket>& ticket,
               const std::optional<Decision>& reply, uint64_t now, Output& output) {
        auto* device = find(event.device);
        if (!device || event.key >= keyCount || !event.modeled) return {Outcome::Forwarded,true,true,false};
        auto& owner = device->owners[event.key];
        if (owner != Owner::None) {
            const auto previous = owner;
            if (event.state == State::Up) owner = Owner::None;
            if (previous == Owner::StockConsumed)
                return {Outcome::StockConsumed,true,false,false}; // release bindings + repeat/longpress cleanup
            if (previous == Owner::LockerOwned)
                return {Outcome::LockerOwned,locked,locked,false};
            if (previous == Owner::Quarantined)
                return {Outcome::Quarantined,false,false,false};
            if (previous == Owner::Consumed)
                return {Outcome::Swallowed,false,false,fatal};
            const bool merged = event.state == State::Up && otherForwarded(event.device,event.key);
            return {merged ? Outcome::Merged : Outcome::Forwarded,true,!merged,false};
        }
        if (event.state != State::Down) return {Outcome::Forwarded,true,true,false};
        if (locked) {
            owner = Owner::LockerOwned;
            return {Outcome::LockerOwned,true,true,false};
        }
        Outcome outcome = Outcome::Forwarded;
        Verdict verdict = Verdict::Pass;
        const bool admitted = pending && ticket && *pending == *ticket &&
                              pending->device == event.device && pending->key == event.key;
        pending.reset();
        if (!locked && !fatal && admitted && ticket && reply && now < ticket->deadline &&
            ticket->device == event.device && ticket->key == event.key && ticket->epoch == epoch && ticket->focus == focus &&
            reply->ticket.device == ticket->device && reply->ticket.epoch == ticket->epoch &&
            reply->ticket.focus == ticket->focus && reply->ticket.sequence == ticket->sequence &&
            reply->ticket.deadline == ticket->deadline && reply->ticket.key == ticket->key &&
            (reply->verdict == Verdict::Pass || reply->verdict == Verdict::Swallow || reply->verdict == Verdict::Replace)) verdict = reply->verdict;
        if (verdict == Verdict::Replace) {
            if (reply->key >= keyCount || reply->key == event.key || targetHeld(reply->key)) {
                verdict = Verdict::Pass; outcome = Outcome::Rejected;
            }
            else {
                const auto delivered = output.replace(reply->key, reply->modifiers, forwardedModifiers());
                if (delivered == Delivery::RejectedNoOutput) { verdict = Verdict::Pass; outcome = Outcome::Rejected; }
                else if (delivered == Delivery::PartialFatal) { fatal = true; outcome = Outcome::PartialFatal; }
                else outcome = Outcome::Replaced;
            }
        }
        owner = verdict == Verdict::Pass ? Owner::Forwarded : Owner::Consumed;
        if (verdict == Verdict::Swallow) outcome = Outcome::Swallowed;
        const bool merged = owner == Owner::Forwarded && otherForwarded(event.device,event.key);
        if (merged && outcome == Outcome::Forwarded) outcome = Outcome::Merged;
        return {outcome,owner == Owner::Forwarded,owner == Owner::Forwarded && !merged,fatal};
    }
    bool targetHeld(uint32_t key) const {
        if (key >= keyCount) return true;
        for (const auto& device : devices)
            if (device.id != 0 && device.owners[key] != Owner::None) return true;
        return false;
    }
    // Must be called synchronously after the stock keybind/seat route on a
    // newly passed press, before focus changes or another event. Pass is a
    // routing proposal, NOT delivery evidence. A stock-consumed key owns its
    // release but contributes nothing to client modifiers/focus-enter keys.
    // stockAdmitted is stock keybinding admission, not whether a redundant
    // client down was emitted. Merged presses still need a stock outcome.
    RouteResult originalOutcome(Event event, RouteResult proposal, bool stockAdmitted) {
        auto* device = find(event.device);
        const bool originalProposal = proposal.outcome == Outcome::Merged || proposal.deliverClient;
        if (!device || event.key >= keyCount || event.state != State::Down || !originalProposal) return proposal;
        if ((device->owners[event.key] == Owner::Forwarded || device->owners[event.key] == Owner::LockerOwned) && !stockAdmitted) {
            device->owners[event.key] = Owner::StockConsumed;
            return {Outcome::StockConsumed,true,false,false};
        }
        return proposal;
    }
    std::array<bool, keyCount> focusEnterKeys() const {
        std::array<bool, keyCount> keys{};
        for (const auto& device : devices)
            if (device.id != 0)
                for (uint32_t key = 0; key < keyCount; ++key)
                    keys[key] = keys[key] || device.owners[key] == Owner::Forwarded;
        return keys;
    }
    std::array<bool, keyCount> lockerEnterKeys() const {
        std::array<bool, keyCount> keys{};
        for (const auto& device : devices)
            if (device.id != 0)
                for (uint32_t key = 0; key < keyCount; ++key)
                    keys[key] = keys[key] || device.owners[key] == Owner::LockerOwned;
        return keys;
    }
    void focusChanged() { pending.reset(); ++focus; ++epoch; }
    void revoke(Output& output) {
        pending.reset();
        ++epoch;
        // Reconcile to the OLD focus before compositor changes to locker focus.
        std::array<bool, keyCount> released{};
        for (auto& device : devices)
            for (uint32_t key = 0; key < keyCount; ++key) {
                auto& owner = device.owners[key];
                if (owner == Owner::Forwarded && !released[key]) {
                    if (!output.release(key)) fatal = true;
                    released[key] = true;
                }
                if (owner != Owner::None) owner = Owner::Quarantined;
            }
        if (!output.reconcileModifiers(0)) fatal = true;
    }
    void lock(Output& output) { locked = true; revoke(output); }
    void unlockConfirmed() { ++epoch; ++focus; locked = false; }
    void disconnect(Output&) { pending.reset(); ++epoch; } // no ownership erasure on transport reset
    void retire(uint64_t id, Output& output) {
        auto* device = find(id);
        if (!device) return;
        ++epoch;
        pending.reset();
        for (uint32_t key = 0; key < keyCount; ++key)
            if (device->owners[key] == Owner::Forwarded && !otherForwarded(id, key) && !output.release(key)) fatal = true;
        *device = Device{};
        if (!output.reconcileModifiers(forwardedModifiers())) fatal = true;
    }
};
} // namespace sledge::hyprland
