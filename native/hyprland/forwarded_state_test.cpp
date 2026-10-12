#include "forwarded_keyboard_state.hpp"
#include "keymap_equivalence.hpp"
#include <cassert>

using namespace sledge::hyprland;
namespace {
struct Sink : Output {
    Delivery replace(uint32_t, uint8_t, uint8_t) override { return Delivery::RejectedNoOutput; }
    bool release(uint32_t) override { return true; }
    bool reconcileModifiers(uint8_t) override { return true; }
};
}
int main() {
    auto* context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    assert(context);
    xkb_rule_names names{}; names.layout = "us";
    auto* keymap = xkb_keymap_new_from_names(context,&names,XKB_KEYMAP_COMPILE_NO_FLAGS);
    assert(keymap);
    auto* equivalent=xkb_keymap_new_from_names(context,&names,XKB_KEYMAP_COMPILE_NO_FLAGS);
    assert(equivalent && equivalent!=keymap && equivalentKeymap(keymap,equivalent));
    // Media-first enumeration must not select aggregate XKB or deny an
    // equivalent verified active typing map.
    assert(!consumerKeymap(equivalent,true,true,true,false,false));
    auto* selected=consumerKeymap(keymap,true,true,true,true,true);
    assert(selected==keymap && equivalentKeymap(selected,equivalent));
    assert(!consumerKeymap(keymap,false,true,true,true,true));
    assert(!equivalentKeymap(keymap,nullptr));
    names.layout="de";
    auto* incompatible=xkb_keymap_new_from_names(context,&names,XKB_KEYMAP_COMPILE_NO_FLAGS);
    assert(incompatible && !equivalentKeymap(keymap,incompatible));
    xkb_keymap_unref(incompatible);
    xkb_keymap_unref(equivalent);
    ForwardedKeyboardState state;
    Router router; Sink output;
    assert(router.add(1) && router.add(2)); router.unlockConfirmed();
    assert(!state.synchronize(router));
    assert(state.setKeymap(keymap));
    Event shift{1,42,State::Down};
    auto proposal = router.route(shift,{},{},1,output);
    router.originalOutcome(shift,proposal,false);
    assert(state.synchronize(router));
    assert(!state.focusEnterKeys()[42] && state.depressed() == 0);
    Event otherShift{2,42,State::Down};
    proposal = router.route(otherShift,{},{},2,output);
    router.originalOutcome(otherShift,proposal,true);
    assert(state.synchronize(router));
    assert(state.focusEnterKeys()[42] && state.depressed() != 0);
    router.focusChanged(); router.disconnect(output);
    assert(state.synchronize(router) && state.depressed() != 0);
    assert(state.setKeymap(keymap) && state.depressed() != 0);
    assert(!state.setKeymap(nullptr) && state.depressed() != 0);
    router.lock(output);
    assert(state.synchronize(router));
    assert(!state.focusEnterKeys()[42] && state.depressed() == 0);
    // Locker-only fresh holds work while locked, but never become app enter
    // state after unlock. Pre-lock quarantined Shift remains absent throughout.
    Event lockerShift{1,54,State::Down};
    proposal=router.route(lockerShift,{},{},3,output);
    router.originalOutcome(lockerShift,proposal,true);
    assert(state.synchronize(router,true) && state.depressed()!=0);
    assert(state.focusEnterKeys()[54] && !state.focusEnterKeys()[42]);
    router.unlockConfirmed();
    assert(state.synchronize(router) && state.depressed() == 0);
    assert(!router.route({1,54,State::Repeat},{},{},4,output).deliverClient);
    assert(!router.route({1,54,State::Up},{},{},5,output).deliverClient);
    xkb_keymap_unref(keymap); xkb_context_unref(context);
}
