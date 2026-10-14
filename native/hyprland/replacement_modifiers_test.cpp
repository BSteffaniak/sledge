#include "replacement_modifiers.hpp"
#include "replacement_emission.hpp"
#include <cassert>
#include <cstdio>

int main() {
    using namespace sledge::hyprland;
    auto* context=xkb_context_new(XKB_CONTEXT_NO_FLAGS);assert(context);
    const xkb_rule_names names{"evdev","pc105","us",nullptr,nullptr};
    auto* map=xkb_keymap_new_from_names(context,&names,XKB_KEYMAP_COMPILE_NO_FLAGS);assert(map);
    auto* synthetic=xkb_state_new(map);auto* physical=xkb_state_new(map);assert(synthetic && physical);
    const auto ctrl=xkb_keymap_mod_get_index(map,XKB_MOD_NAME_CTRL);
    const auto shift=xkb_keymap_mod_get_index(map,XKB_MOD_NAME_SHIFT);
    assert(ctrl<32 && shift<32);
    xkb_state_update_key(physical,42+8,XKB_KEY_DOWN);
    const auto restore=xkb_state_serialize_mods(physical,XKB_STATE_MODS_DEPRESSED);
    assert(restore==(1U<<shift));
    // Independent client projection observes requested/restored protocol masks.
    auto* client=xkb_state_new(map);assert(client);
    for (unsigned transaction=0;transaction<8;++transaction) {
        const auto mask=replacementModifierMask(synthetic,4,0);
        assert(mask==(1U<<ctrl)); // real reused XKB state, not a mask mock
        unsigned downs=0,ups=0;
        const unsigned failAfter=transaction==3 ? 1 : transaction==5 ? 2 : 0;
        const bool complete=emitReplacement(
            [&] { xkb_state_update_mask(client,mask,0,0,0,0,0); },
            [&](bool down) {
                assert(xkb_state_mod_name_is_active(client,XKB_MOD_NAME_CTRL,XKB_STATE_MODS_DEPRESSED)==1);
                assert(xkb_state_mod_name_is_active(client,XKB_MOD_NAME_SHIFT,XKB_STATE_MODS_DEPRESSED)==0);
                if (down) ++downs;else ++ups;
            },
            [&] { xkb_state_update_mask(client,restore,0,0,0,0,0); },failAfter);
        assert(complete==(failAfter==0));
        assert(downs==(failAfter==1 ? 0U : 1U) && ups==downs);
        assert(xkb_state_serialize_mods(client,XKB_STATE_MODS_DEPRESSED)==restore);
        assert(xkb_state_serialize_mods(physical,XKB_STATE_MODS_DEPRESSED)==restore);
    }
    assert(replacementModifierMask(synthetic,0,0)==0);
    assert(replacementModifierMask(synthetic,5,0)==((1U<<ctrl)|(1U<<shift)));
    assert(replacementModifierMask(synthetic,4,0)==(1U<<ctrl));
    xkb_state_unref(client);xkb_state_unref(physical);xkb_state_unref(synthetic);
    xkb_keymap_unref(map);xkb_context_unref(context);
    puts("PASS repeated real XKB modifier transactions: Ctrl reapplied, isolation and restoration");
}
