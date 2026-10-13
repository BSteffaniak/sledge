#include "causal_timing.hpp"
#include <cassert>
#include <thread>
using namespace sledge::hyprland;
int main() {
    WirePacket packet{};
    // Cross-process receive may occur inside the observed syscall interval.
    CausalTiming interval;
    interval.record(CausalTiming::Phase::OutcomeSendStart,packet,15,true,100);
    interval.record(CausalTiming::Phase::OutcomeSend,packet,15,true,200);
    CausalTiming::Record startRecord,returnRecord;
    assert(interval.read(0,startRecord) && interval.read(1,returnRecord));
    const auto legal=[&](uint64_t received) {
        return startRecord.ok && returnRecord.ok && received>=startRecord.at && returnRecord.at>=startRecord.at;
    };
    assert(legal(150) && legal(250) && !legal(99));
    // Return alone cannot establish the lower bound; incomplete traces retain
    // the missing phase rather than synthesizing a timestamp.
    CausalTiming partial;
    partial.record(CausalTiming::Phase::EventSend,packet,15,false,200);
    assert(partial.json().find("event_send_start")==std::string::npos);
    assert(interval.json().find("outcome_send_start")!=std::string::npos);
    CausalTiming timing;
    wire::put(packet,16,8,11);wire::put(packet,24,8,12);wire::put(packet,32,8,13);
    wire::put(packet,40,8,1000);wire::put(packet,48,8,14);
    for (unsigned phase=0;phase<7;++phase)
        timing.record(static_cast<CausalTiming::Phase>(phase),packet,15,true,100+phase);
    for (unsigned i=0;i<7;++i) {
        CausalTiming::Record r;assert(timing.read(i,r));
        assert(r.id==i+1 && r.at==100+i && r.phase==static_cast<CausalTiming::Phase>(i));
        assert(r.nonce==14 && r.epoch==12 && r.sequence==13 && r.generation==11 && r.connection==15 && r.deadline==1000);
    }
    // A stale scope is observed verbatim, never conflated with a fresh nonce.
    wire::put(packet,48,8,99);timing.record(CausalTiming::Phase::EventSend,packet,16,false,108);
    CausalTiming::Record stale;assert(timing.read(7,stale) && stale.nonce==99 && !stale.ok);
    assert(timing.json().find("\"nonce\":99")!=std::string::npos);
    for (unsigned i=0;i<CausalTiming::capacity;++i) timing.record(CausalTiming::Phase::EventEnqueue,packet,16,true,200+i);
    assert(timing.metadata().find("\"overwritten\":8")!=std::string::npos);
    CausalTiming::Cut cut;
    assert(!CausalTiming::parse("0 1 0 13",cut));
    assert(!CausalTiming::parse("0 1 2 1",cut));
    assert(!CausalTiming::parse("0 18446744073709551616 0 1",cut));
    assert(!CausalTiming::parse("-1 1 0 1",cut));
    assert(!CausalTiming::parse("0 1 0 1 trailing",cut));
    assert(CausalTiming::parse("0 8 0 8",cut));
    assert(timing.page(cut).find("\"page_complete\":false")!=std::string::npos);
    const auto end=timing.attempted(),start=end-CausalTiming::capacity;
    for (auto cursor=start;cursor<end;cursor+=12) {
        const auto page=timing.page({start,end,cursor,12});
        assert(page.size()<8192 && page.find("\"missing_ids\":[]")!=std::string::npos);
    }
    CausalTiming worst;
    WirePacket maximum{};
    for (unsigned offset : {16,24,32,40,48}) wire::put(maximum,offset,8,UINT64_MAX);
    for (unsigned i=0;i<12;++i) worst.record(CausalTiming::Phase::DecisionReceive,maximum,UINT64_MAX,false,UINT64_MAX);
    assert(worst.page({0,12,0,12}).size()<8192 && worst.metadata().size()<1024);
    assert(worst.query("sledge:causal-timing 0 12 0 12")==worst.page({0,12,0,12}));
    CausalTiming controls;
    for (const auto opcode : {7U,8U,9U}) {
        WirePacket control=maximum;
        wire::put(control,6,2,opcode);wire::put(control,64,4,UINT32_MAX);
        controls.record(CausalTiming::Phase::ControlEnqueue,control,17,true,100);
        controls.record(CausalTiming::Phase::ControlSendStart,control,17,true,200);
        controls.record(CausalTiming::Phase::ControlSend,control,17,true,300);
    }
    assert(controls.json().find("\"kind\":\"control\"")!=std::string::npos);
    assert(controls.json().find("\"control_value\":4294967295")!=std::string::npos);
    CausalTiming::Record controlRecord;
    assert(controls.read(3,controlRecord) && controlRecord.opcode==8 && controlRecord.connection==17);
    assert(controls.page({0,9,0,12}).size()+500<8192);
    WirePacket expired=maximum;wire::put(expired,6,2,7);wire::put(expired,64,4,8);
    controls.record(CausalTiming::Phase::ControlSend,expired,16,false,UINT64_MAX);
    assert(controls.read(9,controlRecord) && !controlRecord.ok && controlRecord.controlValue==8 && controlRecord.connection==16);
    assert(worst.page({0,13,0,12}).find("error")!=std::string::npos);
    assert(worst.query("sledge:causal-timing-extra").find("error")!=std::string::npos);
    // Worst-width identifiers and counters still fit: all fields contribute at
    // most 20 decimal bytes, so replacing these small IDs adds < 500 bytes.
    const auto worstBytes=worst.page({0,12,0,12}).size();
    assert(worstBytes+500<8192);
    const auto oldEnd=timing.attempted(),oldStart=oldEnd-CausalTiming::capacity;
    for (unsigned i=0;i<12;++i) timing.record(CausalTiming::Phase::EventSend,packet,16,true,400+i);
    assert(timing.page({oldStart,oldEnd,oldStart,12}).find("\"page_complete\":false")!=std::string::npos);
    CausalTiming::Record overwritten;assert(timing.read(0,overwritten) && overwritten.id>CausalTiming::capacity);
    // Concurrent worker/callback recording and status snapshots remain race-free;
    // each returned record is an entire scope, never mixed atomic field values.
    std::thread worker([&] { for (unsigned i=0;i<5000;++i) timing.record(CausalTiming::Phase::EventSend,packet,16,true,300+i); });
    for (unsigned i=0;i<5000;++i) {
        CausalTiming::Record r;
        if (timing.read(i%CausalTiming::capacity,r)) assert(r.nonce==99 && r.connection==16);
        if (i%100==0) {
            const auto current=timing.attempted(),begin=current>CausalTiming::capacity ? current-CausalTiming::capacity : 0;
            const auto concurrentPage=timing.page({begin,current,begin,12});
            assert(concurrentPage.size()<8192 && concurrentPage.find("\"missing_ids\":")!=std::string::npos);
        }
    }
    worker.join();
    // Enqueue observation occurs only after mailbox admission, before publish.
    DecisionMailbox mailbox;unsigned observed=0;WirePacket reply{};
    const auto observer=[](void* count,const WirePacket&) noexcept { ++*static_cast<unsigned*>(count); };
    assert(!mailbox.decide(packet,0,reply,observer,&observed) && observed==0);
}
