#include "stock_worker.hpp"
#include "hook_bytes.hpp"
#include <cassert>
#include <chrono>
#include <cstdio>

using namespace sledge::hyprland;
namespace {
void framing() {
    WirePacket hello{}, ack{};
    assert(wire::hello(hello,17,"sdk-hash","closure+digest"));
    assert(wire::header(hello,1,400));
    assert(wire::get(hello,64,8)==0);
    ack = hello; wire::put(ack,6,2,2);
    assert(wire::helloAck(hello,ack));
    ack[511]=1; assert(!wire::helloAck(hello,ack)); ack[511]=0;
    ack[64]=1; assert(!wire::helloAck(hello,ack));
    WirePacket event{}, decision{};
    event[0]='S'; event[1]='H'; event[2]='P'; event[3]='1';
    wire::put(event,4,2,1); wire::put(event,6,2,3); wire::put(event,8,4,32);
    wire::put(event,16,8,1); wire::put(event,24,8,2); wire::put(event,32,8,3);
    wire::put(event,40,8,4); wire::put(event,48,8,17);
    decision=event; wire::put(decision,6,2,4); wire::put(decision,8,4,16);
    assert(wire::decision(event,decision));
    wire::put(decision,64,4,2); wire::put(decision,68,4,30); wire::put(decision,72,4,4);
    assert(wire::decision(event,decision));
    for (const unsigned offset : {12,16,24,32,40,48,56,76,80,511}) {
        auto corrupt = decision; corrupt[offset]^=1;
        assert(!wire::decision(event,corrupt));
    }
    wire::put(decision,68,4,768); assert(!wire::decision(event,decision));
    std::array<uint8_t,64> bytes{}; HookBytes snapshot;
    snapshot.snapshot(bytes.data()); assert(snapshot.restored());
    bytes[6]=1; assert(!snapshot.restored()); bytes[6]=0; snapshot.requireRestored();
}
void connection() {
    char directory[]="/tmp/sledge-native-worker-XXXXXX";
    assert(mkdtemp(directory));
    assert(chmod(directory,0700)==0);
    const auto path=std::string(directory)+"/peer.sock";
    const auto listener=socket(AF_UNIX,SOCK_SEQPACKET|SOCK_CLOEXEC,0);
    assert(listener>=0);
    sockaddr_un address{}; address.sun_family=AF_UNIX;
    std::memcpy(address.sun_path,path.c_str(),path.size()+1);
    assert(bind(listener,reinterpret_cast<sockaddr*>(&address),sizeof(address))==0);
    assert(chmod(path.c_str(),0600)==0);
    assert(listen(listener,1)==0);
    std::thread server([&] {
        pollfd pending{listener,POLLIN,0};
        assert(poll(&pending,1,1000)>0);
        const auto client=accept4(listener,nullptr,nullptr,SOCK_CLOEXEC);
        assert(client>=0);
        WirePacket hello{};
        assert(recv(client,hello.data(),hello.size(),MSG_TRUNC)==512);
        assert(wire::header(hello,1,400) && wire::get(hello,64,8)==0);
        wire::put(hello,6,2,2);
        assert(send(client,hello.data(),hello.size(),MSG_NOSIGNAL)==512);
        for (const unsigned state : {1U,2U,0U}) {
            WirePacket event{};
            assert(recv(client,event.data(),event.size(),MSG_TRUNC)==512);
            assert(wire::header(event,3,32) && wire::get(event,68,4)==state);
            auto decision=event;
            for (unsigned i=64;i<512;++i) decision[i]=0;
            wire::put(decision,6,2,4); wire::put(decision,8,4,16);
            assert(send(client,decision.data(),decision.size(),MSG_NOSIGNAL)==512);
            WirePacket outcome{};
            assert(recv(client,outcome.data(),outcome.size(),MSG_TRUNC)==512);
            assert(wire::header(outcome,5,24) && wire::correlated(event,outcome));
            assert(wire::get(outcome,76,4)==state);
        }
        // Keep socket alive until worker closes; no asynchronous injection.
        char byte{}; (void)recv(client,&byte,1,0);
        close(client);
    });
    DecisionMailbox mailbox; WirePacket hello{}, event{}, decision{};
    assert(wire::hello(hello,17,"sdk-hash","closure+digest"));
    {
        StockWorker worker(mailbox,path,hello);
        const auto limit=std::chrono::steady_clock::now()+std::chrono::seconds(1);
        while (!worker.connected() && std::chrono::steady_clock::now()<limit) std::this_thread::yield();
        assert(worker.connected());
        event=hello; wire::put(event,6,2,3); wire::put(event,8,4,32);
        for (unsigned i=64; i<512; ++i) event[i]=0;
        wire::put(event,16,8,1); wire::put(event,24,8,2); wire::put(event,32,8,3);
        for (const unsigned state : {1U,2U,0U}) {
            wire::put(event,32,8,wire::get(event,32,8)+1);wire::put(event,68,4,state);
            const auto deadline=monotonicNs()+10'000'000;
            wire::put(event,40,8,deadline);
            assert(mailbox.decide(event,deadline,decision));
            assert(wire::decision(event,decision));
            auto outcome=event;
            for (unsigned i=64;i<512;++i) outcome[i]=0;
            wire::put(outcome,6,2,5);wire::put(outcome,8,4,24);wire::put(outcome,76,4,state);
            assert(worker.notify(outcome));
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    server.join(); close(listener);
    assert(unlink(path.c_str())==0); assert(rmdir(directory)==0);
}
}
int main() { framing(); connection(); }
