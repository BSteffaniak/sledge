#include "stock_wire.hpp"
#include "initial_reconciliation.hpp"
#include "focus_identity.hpp"
#include <cassert>
#include <cstdio>
using namespace sledge::hyprland;
int main() {
 wire::Scope s{0,6,5,10000000,17};WirePacket p{};
 uint64_t generation=0,nonce=0,sequence=0,epoch=0;
 WirePacket event{},reply{};
 assert(wire::eventPacket(event,{1,1,1,100,1},100,0,1,32,1));
 reply=event;wire::put(reply,6,2,4);wire::put(reply,8,4,16);
 for (unsigned i=64;i<512;++i) reply[i]=0;
 wire::put(reply,64,4,2);wire::put(reply,68,4,28);wire::put(reply,72,4,4);
 assert(!wire::decision(event,reply,false));
 assert(wire::decision(event,reply,true));
 wire::put(reply,64,4,1);wire::put(reply,68,8,0);
 assert(!wire::decision(event,reply,true)); // Up never swallows
 wire::put(reply,64,4,2);wire::put(reply,68,4,28);wire::put(reply,72,4,4);
 wire::put(event,68,4,2);assert(!wire::decision(event,reply,true));
 wire::put(event,68,4,0);wire::put(event,64,4,30);assert(!wire::decision(event,reply,true));
 wire::put(event,68,4,1);assert(wire::decision(event,reply,false));
 assert(bindConnection(1,17,generation,nonce,sequence,epoch));
 for (unsigned i=1;i<=4;++i) {
  assert(wire::control(p,i%2 ? wire::Opcode::Lock : wire::Opcode::Reset,
      {0,6,++sequence,10000000,nonce},i%2 ? 0 : 2));
  assert(wire::get(p,32,8)==i);
 }
 epoch=6;
 assert(!bindConnection(1,17,generation,nonce,sequence,epoch));
 assert(wire::control(p,wire::Opcode::Reset,{0,epoch,++sequence,10000000,nonce},8));
 assert(wire::get(p,32,8)==5);
 assert(bindConnection(2,18,generation,nonce,sequence,epoch));
 assert(wire::control(p,wire::Opcode::Reset,{0,7,++sequence,10000000,nonce},8));
 assert(wire::get(p,32,8)==1 && wire::get(p,48,8)==18);
 assert(wire::control(p,wire::Opcode::Reset,s,8));
 assert(wire::header(p,7,8) && wire::get(p,64,4)==8 && wire::zero(p,68,512));
 auto write=[&] { assert(std::fwrite(p.data(),1,p.size(),stdout)==512); };
 write();
 assert(!wire::control(p,wire::Opcode::Lock,s,8));
 assert(wire::control(p,wire::Opcode::Lock,s,0));write();
 s.device=1;
 assert(!wire::control(p,wire::Opcode::Reset,s,8));
 for (unsigned state=0;state<3;++state) {
  assert(wire::eventPacket(p,s,30,state,123,4,2));write();
  assert(encodeFocus(p,2,"SledgeFixture"));write();
 }
}
