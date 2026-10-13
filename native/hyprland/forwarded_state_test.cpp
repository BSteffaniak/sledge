#include "output_reconciliation.hpp"
#include "forwarded_keyboard_state.hpp"
#include "keymap_equivalence.hpp"
#include <string>
#include <cstdlib>
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
    {
        Router mixed;assert(mixed.add(10) && mixed.add(20));mixed.unlockConfirmed();
        const Event alt{10,56,State::Down};
        const auto down=mixed.route(alt,{},{},1,output);mixed.originalOutcome(alt,down,true);
        const xkb_rule_names sameNames{nullptr,nullptr,"us",nullptr,nullptr};
        auto* second=xkb_keymap_new_from_names(context,&sameNames,XKB_KEYMAP_COMPILE_NO_FLAGS);
        assert(second && equivalentKeymap(keymap,second));
        ForwardedKeyboardState projected;
        assert(projected.setKeymap(keymap) && projected.prepare(second) && projected.synchronize(mixed));
        const auto mask=projected.depressed();assert(mask!=0);
        assert(projected.select(second) && projected.synchronize(mixed));
        assert(projected.depressed()==mask && projected.focusEnterKeys()[56]);
        assert(projected.select(keymap) && projected.synchronize(mixed) && projected.depressed()==mask);
        const Event up{10,56,State::Up};
        const auto released=mixed.route(up,{},{},2,output);mixed.originalOutcome(up,released,true);
        assert(projected.synchronize(mixed) && projected.depressed()==0);
        xkb_keymap_unref(second);
    }
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
    names.layout="de";
    auto* german=xkb_keymap_new_from_names(context,&names,XKB_KEYMAP_COMPILE_NO_FLAGS);
    assert(german && state.prepare(german));
    assert(state.select(german) && state.synchronize(router));
    auto* expected=xkb_state_new(german);assert(expected);
    xkb_state_update_key(expected,50,XKB_KEY_DOWN);
    assert(state.depressed()==xkb_state_serialize_mods(expected,XKB_STATE_MODS_DEPRESSED));
    char* serialized=xkb_keymap_get_as_string(german,XKB_KEYMAP_FORMAT_TEXT_V1);assert(serialized);
    std::string alternate(serialized);std::free(serialized);
    std::size_t mapping=0;unsigned replaced=0;
    while ((mapping=alternate.find("modifier_map Shift",mapping))!=std::string::npos) {
        alternate.replace(mapping,18,"modifier_map Mod3 ");mapping+=18;++replaced;
    }
    std::size_t action=0;
    while ((action=alternate.find("modifiers=Shift",action))!=std::string::npos) {
        alternate.replace(action,15,"modifiers=Mod3 ");action+=15;
    }
    assert(replaced);
    auto* shifted=xkb_keymap_new_from_string(context,alternate.c_str(),XKB_KEYMAP_FORMAT_TEXT_V1,XKB_KEYMAP_COMPILE_NO_FLAGS);
    assert(shifted && state.prepare(shifted) && state.select(shifted));
    auto* shiftedExpected=xkb_state_new(shifted);assert(shiftedExpected);
    xkb_state_update_key(shiftedExpected,50,XKB_KEY_DOWN);
    assert(state.depressed()==xkb_state_serialize_mods(shiftedExpected,XKB_STATE_MODS_DEPRESSED));
    assert(state.depressed()!=xkb_state_serialize_mods(expected,XKB_STATE_MODS_DEPRESSED));
    xkb_state_unref(shiftedExpected);xkb_keymap_unref(shifted);
    names.layout="us,de";
    auto* grouped=xkb_keymap_new_from_names(context,&names,XKB_KEYMAP_COMPILE_NO_FLAGS);
    assert(grouped && state.prepare(grouped) && state.select(grouped,1) && state.group()==1);
    assert(state.focusEnterKeys()[42]);
    xkb_keymap_unref(grouped);
    assert(state.select(keymap) && state.synchronize(router));
    assert(state.focusEnterKeys()[42] && state.depressed()!=0);
    xkb_state_unref(expected);xkb_keymap_unref(german);
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
    // Genuine CapsLock is native forwarded history, not an outstanding hold.
    // Re-selection and map projection must preserve it, never erase to admit.
    router.route({1,54,State::Up},{},{},6,output);
    const Event capsDown{1,58,State::Down},capsUp{1,58,State::Up};
    proposal=router.route(capsDown,{},{},7,output);
    router.originalOutcome(capsDown,proposal,true);assert(state.synchronize(router));
    proposal=router.route(capsUp,{},{},8,output);
    router.originalOutcome(capsUp,proposal,true);assert(state.synchronize(router));
    const auto capsMask=state.locked();assert(capsMask!=0);
    assert(state.select(keymap) && state.locked()==capsMask);
    assert(outputReconciled(true,true,false,state.depressed()!=0,false));
    names.layout="us";
    auto* capsMap=xkb_keymap_new_from_names(context,&names,XKB_KEYMAP_COMPILE_NO_FLAGS);
    assert(capsMap && state.prepare(capsMap) && state.select(capsMap) && state.locked()==capsMask);
    assert(state.select(keymap) && state.locked()==capsMask);
    xkb_keymap_unref(capsMap);
    xkb_keymap_unref(keymap); xkb_context_unref(context);
}
