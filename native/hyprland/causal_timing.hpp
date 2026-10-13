#pragma once
#include "stock_wire.hpp"
#include <algorithm>
#include <charconv>
#include <string>
namespace sledge::hyprland {
// Fixed observation ring, never an admission authority. One nonblocking attempt
// per slot; status readers can cause observable drops, never callback waiting.
class CausalTiming {
public:
    enum class Phase : unsigned { EventEnqueue,EventSend,DecisionReceive,MailboxPublish,RouteComplete,OutcomeEnqueue,OutcomeSend,EventSendStart,OutcomeSendStart,ControlEnqueue,ControlSendStart,ControlSend };
    static constexpr std::array<const char*,12> names{"event_enqueue","event_send","decision_receive","mailbox_publish","route_complete","outcome_enqueue","outcome_send","event_send_start","outcome_send_start","control_enqueue","control_send_start","control_send"};
    static constexpr unsigned capacity=2048;
    struct Record { uint64_t id=0,at=0,nonce=0,epoch=0,sequence=0,generation=0,connection=0,deadline=0;Phase phase{};bool ok=false;uint64_t opcode=0,controlValue=0; };
private:
    struct Slot { mutable std::atomic_flag busy=ATOMIC_FLAG_INIT;Record value{}; };
    std::array<Slot,capacity> slots{};
    std::atomic<uint64_t> next{0},dropped{0};
    static_assert(std::atomic<uint64_t>::is_always_lock_free);
public:
    void record(Phase phase,const WirePacket& packet,uint64_t connection,bool ok=true,uint64_t at=monotonicNs()) noexcept {
        const auto index=next.fetch_add(1,std::memory_order_relaxed);
        auto& slot=slots[index%capacity];
        if (slot.busy.test_and_set(std::memory_order_acquire)) { dropped.fetch_add(1,std::memory_order_relaxed);return; }
        // A delayed writer may encounter a newer wrap of this slot. Never
        // replace newer evidence with an older reservation.
        if (slot.value.id>index+1) {
            dropped.fetch_add(1,std::memory_order_relaxed);slot.busy.clear(std::memory_order_release);return;
        }
        slot.value={index+1,at,wire::get(packet,48,8),wire::get(packet,24,8),wire::get(packet,32,8),
            wire::get(packet,16,8),connection,wire::get(packet,40,8),phase,ok,wire::get(packet,6,2),
            phase>=Phase::ControlEnqueue ? wire::get(packet,64,4) : 0};
        slot.busy.clear(std::memory_order_release);
    }
    bool read(unsigned index,Record& result) const noexcept {
        if (index>=capacity) return false;
        const auto& slot=slots[index];
        if (slot.busy.test_and_set(std::memory_order_acquire)) return false;
        result=slot.value;slot.busy.clear(std::memory_order_release);return result.id!=0;
    }
    struct Cut { uint64_t start=0,end=0,cursor=0,limit=12; };
    uint64_t attempted() const noexcept { return next.load(std::memory_order_acquire); }
    static bool parse(std::string_view text,Cut& cut) noexcept {
        std::array<uint64_t,4> fields{};
        for (auto& value : fields) {
            while (!text.empty() && text.front()==' ') text.remove_prefix(1);
            if (text.empty()) return false;
            const auto result=std::from_chars(text.data(),text.data()+text.size(),value);
            if (result.ec!=std::errc{} || result.ptr==text.data()) return false;
            text.remove_prefix(result.ptr-text.data());
            if (!text.empty() && text.front()!=' ') return false;
        }
        while (!text.empty() && text.front()==' ') text.remove_prefix(1);
        cut={fields[0],fields[1],fields[2],fields[3]};
        return text.empty() && cut.start<=cut.cursor && cut.cursor<=cut.end &&
            cut.end-cut.start<=capacity && cut.limit>=1 && cut.limit<=12;
    }
    std::string metadata() const {
        const auto end=attempted();
        return "{\"version\":1,\"unit\":\"ns\",\"capacity\":2048,\"attempted\":"+std::to_string(end)+
            ",\"overwritten\":"+std::to_string(end>capacity ? end-capacity : 0)+",\"dropped\":"+
            std::to_string(dropped.load(std::memory_order_relaxed))+",\"snapshot_start\":"+
            std::to_string(end>capacity ? end-capacity : 0)+",\"snapshot_end\":"+std::to_string(end)+"}";
    }
    std::string page(Cut cut) const {
        const auto observed=attempted();
        if (cut.start>cut.cursor || cut.cursor>cut.end || cut.end>observed || cut.end-cut.start>capacity ||
            cut.limit<1 || cut.limit>12) return "{\"error\":\"invalid causal cursor\"}";
        const auto stop=cut.cursor+std::min(cut.limit,cut.end-cut.cursor);
        std::string out="{\"version\":1,\"unit\":\"ns\",\"capacity\":2048,\"snapshot_start\":"+
            std::to_string(cut.start)+",\"snapshot_end\":"+std::to_string(cut.end)+",\"cursor\":"+
            std::to_string(cut.cursor)+",\"next_cursor\":"+std::to_string(stop)+",\"observed_end\":"+
            std::to_string(observed)+",\"overwritten\":"+std::to_string(observed>capacity ? observed-capacity : 0)+
            ",\"dropped\":"+std::to_string(dropped.load(std::memory_order_relaxed))+
            ",\"done\":"+(stop==cut.end ? "true" : "false")+",\"records\":[";
        std::string missing="[";bool comma=false,missingComma=false;
        for (uint64_t i=cut.cursor;i<stop;++i) {
            Record r;
            if (!read(i%capacity,r) || r.id!=i+1) {
                if (missingComma) missing+=",";
                missingComma=true;missing+=std::to_string(i+1);continue;
            }
            if (comma) out+=",";
            comma=true;
            out+="{\"id\":"+std::to_string(r.id)+",\"phase\":\""+names[static_cast<unsigned>(r.phase)]+
                "\",\"at_ns\":"+std::to_string(r.at)+",\"nonce\":"+std::to_string(r.nonce)+",\"epoch\":"+
                std::to_string(r.epoch)+",\"sequence\":"+std::to_string(r.sequence)+",\"generation\":"+
                std::to_string(r.generation)+",\"connection_generation\":"+std::to_string(r.connection)+
                ",\"deadline_ns\":"+std::to_string(r.deadline)+",\"ok\":"+(r.ok ? "true" : "false")+
                ",\"kind\":\""+(r.phase>=Phase::ControlEnqueue ? "control" : "event")+"\",\"opcode\":"+
                std::to_string(r.opcode)+",\"control_value\":"+std::to_string(r.controlValue)+"}";
        }
        return out+"],\"missing_ids\":"+missing+"],\"page_complete\":"+(missingComma ? "false" : "true")+"}";
    }
    std::string json() const {
        const auto end=attempted();
        return page({end>12 ? end-12 : 0,end,end>12 ? end-12 : 0,12});
    }
    std::string query(std::string_view arguments) const {
        const auto separator=arguments.find(' ');
        if (arguments.starts_with("sledge:causal-timing")) {
            constexpr std::string_view command="sledge:causal-timing";
            if (arguments.size()>command.size() && arguments[command.size()]!=' ')
                return "{\"error\":\"invalid causal command\"}";
            arguments=separator==std::string_view::npos ? std::string_view{} : arguments.substr(separator+1);
        }
        if (arguments.empty()) {
            const auto end=attempted();const auto start=end>capacity ? end-capacity : 0;
            return page({start,end,start,12});
        }
        Cut cut;if (!parse(arguments,cut)) return "{\"error\":\"expected START END CURSOR LIMIT (1..12)\"}";
        return page(cut);
    }
};
}
