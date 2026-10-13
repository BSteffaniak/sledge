#pragma once
namespace sledge::hyprland {
// Scope is stack-owned on the compositor thread. Wrapper entry alone is never
// evidence: only a valid generated protocol send returning completes it.
class EnterCompletion {
    bool sent=false;
public:
    void completed(bool valid) noexcept { sent=sent || valid; }
    bool observed() const noexcept { return sent; }
};
}
