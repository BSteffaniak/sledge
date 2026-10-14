#include "stock_worker.hpp"
#include "hook_bytes.hpp"
#include "focus_identity.hpp"
#include <cassert>
#include <chrono>
#include <cstdio>

using namespace sledge::hyprland;
namespace {
void framing() {
    WirePacket focused{};
    assert(encodeFocus(focused,1,"org.example.App") && wire::get(focused,88,2)==1);
    assert(wire::get(focused,90,2)==15 && wire::get(focused,8,4)==47);
    assert(!validFocusIdentity(std::string(257,'a')));
    assert(!validFocusIdentity(std::string("a\0b",3)));
    assert(!validFocusIdentity("\xC0\xAF"));
    assert(!validFocusIdentity("\xED\xA0\x80"));
    assert(validFocusIdentity("app.\xE2\x82\xAC"));
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
    wire::put(event,68,4,1); // actual fresh Down request, not zero-filled Up
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
        WirePacket reset{};
        assert(recv(client,reset.data(),512,MSG_TRUNC)==512);
        assert(wire::header(reset,7,8) && wire::get(reset,64,4)==3);
        WirePacket boundary{};
        assert(recv(client,boundary.data(),512,MSG_TRUNC)==512);
        assert(wire::header(boundary,7,8) && wire::get(boundary,64,4)==8 && wire::get(boundary,16,8)==0);
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
    CausalTiming timing;
    assert(wire::hello(hello,17,"sdk-hash","closure+digest"));
    {
        StockWorker worker(mailbox,path,hello,&timing);
        const auto limit=std::chrono::steady_clock::now()+std::chrono::seconds(1);
        while (!worker.connected() && std::chrono::steady_clock::now()<limit) std::this_thread::yield();
        assert(worker.connected());
        event=hello; wire::put(event,6,2,3); wire::put(event,8,4,32);
        for (unsigned i=64; i<512; ++i) event[i]=0;
        wire::put(event,16,8,1); wire::put(event,24,8,2); wire::put(event,32,8,3);
        auto boundary=event;
        wire::put(boundary,6,2,7);wire::put(boundary,8,4,8);wire::put(boundary,16,8,0);
        wire::put(boundary,40,8,monotonicNs()+10'000'000);wire::put(boundary,64,4,8);
        auto reset=boundary;wire::put(reset,64,4,3);
        assert(worker.notify(reset));
        assert(worker.connected() && worker.reasonCount(StockWorker::Reason::RequestedClose)==0);
        assert(worker.notify(boundary));
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
        worker.requestPark();
        const auto parkDeadline=std::chrono::steady_clock::now()+std::chrono::seconds(1);
        while (!worker.idleParked() && std::chrono::steady_clock::now()<parkDeadline) std::this_thread::yield();
        assert(worker.idleParked());
        std::array<unsigned,12> observed{};
        for (unsigned i=0;i<CausalTiming::capacity;++i) {
            CausalTiming::Record r;if (!timing.read(i,r)) continue;
            assert(r.nonce==17 && r.connection==1 && r.ok);
            assert(r.generation==(r.phase>=CausalTiming::Phase::ControlEnqueue ? 0 : 1));
            ++observed[static_cast<unsigned>(r.phase)];
        }
        assert(observed[1]==3 && observed[2]==3 && observed[3]==3 && observed[5]==3 && observed[6]==3 && observed[7]==3 && observed[8]==3 && observed[9]==2 && observed[10]==2 && observed[11]==2);
        assert(!worker.notify(boundary));
        bool rejectedControl=false;
        for (unsigned i=0;i<CausalTiming::capacity;++i) {
            CausalTiming::Record r;
            if (timing.read(i,r) && r.phase==CausalTiming::Phase::ControlEnqueue && !r.ok) {
                assert(r.opcode==7 && r.controlValue==8 && r.nonce==17);
                rejectedControl=true;
            }
        }
        assert(rejectedControl);
        worker.invalidate();
        assert(!worker.connected() && worker.reasonCount(StockWorker::Reason::RequestedClose)==1);
    }
    server.join(); close(listener);
    assert(unlink(path.c_str())==0); assert(rmdir(directory)==0);
}
void delayedHello() {
    char directory[]="/hdd/sledge-worker-late-XXXXXX";
    assert(mkdtemp(directory));
    const std::string path=std::string(directory)+"/peer.sock";
    const int listener=socket(AF_UNIX,SOCK_SEQPACKET|SOCK_CLOEXEC,0);
    assert(listener>=0);
    sockaddr_un address{};address.sun_family=AF_UNIX;
    std::memcpy(address.sun_path,path.c_str(),path.size()+1);
    assert(bind(listener,reinterpret_cast<sockaddr*>(&address),sizeof(address))==0);
    assert(chmod(path.c_str(),0600)==0 && listen(listener,1)==0);
    std::jthread server([&] {
        const int client=accept4(listener,nullptr,nullptr,SOCK_CLOEXEC);
        assert(client>=0);
        WirePacket ack{};assert(recv(client,ack.data(),512,MSG_TRUNC)==512);
        std::this_thread::sleep_for(std::chrono::milliseconds(30));
        wire::put(ack,6,2,2);
        (void)send(client,ack.data(),512,MSG_NOSIGNAL);
        close(client);
    });
    DecisionMailbox mailbox;WirePacket hello{};
    assert(wire::hello(hello,100,"sdk-hash","closure+digest"));
    {
        StockWorker worker(mailbox,path,hello);
        const auto limit=std::chrono::steady_clock::now()+std::chrono::seconds(1);
        while (!worker.reasonCount(StockWorker::Reason::HelloReceiveTimeout) && std::chrono::steady_clock::now()<limit)
            std::this_thread::yield();
        assert(worker.reasonCount(StockWorker::Reason::HelloReceiveTimeout)==1);
        assert(!worker.connected() && worker.reasonCount(StockWorker::Reason::Authenticated)==0);
    }
    server.join();close(listener);
    assert(unlink(path.c_str())==0 && rmdir(directory)==0);
}
void disconnectedDrain() {
    char directory[]="/tmp/sledge-worker-drain-XXXXXX";
    assert(mkdtemp(directory));assert(chmod(directory,0700)==0);
    const auto path=std::string(directory)+"/peer.sock";
    const auto listener=socket(AF_UNIX,SOCK_SEQPACKET|SOCK_CLOEXEC,0);assert(listener>=0);
    sockaddr_un address{};address.sun_family=AF_UNIX;
    std::memcpy(address.sun_path,path.c_str(),path.size()+1);
    assert(bind(listener,reinterpret_cast<sockaddr*>(&address),sizeof(address))==0);
    assert(chmod(path.c_str(),0600)==0);assert(listen(listener,1)==0);
    std::atomic<bool> received{false},finished{false};
    std::thread server([&] {
        pollfd incoming{listener,POLLIN,0};assert(poll(&incoming,1,3000)>0);
        const auto client=accept4(listener,nullptr,nullptr,SOCK_CLOEXEC);assert(client>=0);
        WirePacket packet{};assert(recv(client,packet.data(),512,MSG_TRUNC)==512);
        wire::put(packet,6,2,2);assert(send(client,packet.data(),512,MSG_NOSIGNAL)==512);
        assert(recv(client,packet.data(),512,MSG_TRUNC)==512);received.store(true);
        while (!finished.load()) std::this_thread::yield();
        close(client);
    });
    DecisionMailbox mailbox;
    WirePacket hello{};assert(wire::hello(hello,100,"sdk-hash","closure+digest"));
    {
        StockWorker worker(mailbox,path,hello);
        const auto limit=std::chrono::steady_clock::now()+std::chrono::seconds(3);
        while (!worker.connected() && std::chrono::steady_clock::now()<limit) std::this_thread::yield();
        assert(worker.connected());
        WirePacket event=hello;
        for (unsigned i=64;i<512;++i) event[i]=0;
        wire::put(event,6,2,3);wire::put(event,8,4,32);
        wire::put(event,16,8,1);wire::put(event,24,8,1);wire::put(event,32,8,1);
        wire::put(event,40,8,monotonicNs()+9'000'000);
        std::thread caller([&] { WirePacket reply{};(void)mailbox.decide(event,wire::get(event,40,8),reply); });
        while (!received.load() && std::chrono::steady_clock::now()<limit) std::this_thread::yield();
        assert(received.load());
        auto receipt=event;wire::put(receipt,6,2,5);wire::put(receipt,8,4,24);
        assert(worker.notify(receipt));
        worker.requestPark();
        assert(!worker.retireGenerationForDrain(worker.generation(),false));
        assert(!worker.retireGenerationForDrain(worker.generation()+1,true));
        caller.join(); // timed-out transaction; queued receipt never sent
        while (worker.connected() && std::chrono::steady_clock::now()<limit) std::this_thread::yield();
        assert(!worker.connected() && mailbox.idle() && !worker.idleParked());
        assert(worker.retireGenerationForDrain(worker.generation(),true));
        while (!worker.idleParked() && std::chrono::steady_clock::now()<limit) std::this_thread::yield();
        assert(worker.idleParked());finished.store(true);
    }
    server.join();close(listener);assert(unlink(path.c_str())==0);assert(rmdir(directory)==0);
}
void reconnect() {
    char directory[]="/hdd/sledge-wave6-worker-XXXXXX";
    assert(mkdtemp(directory));
    const std::string path=std::string(directory)+"/peer.sock";
    const int listener=socket(AF_UNIX,SOCK_SEQPACKET|SOCK_CLOEXEC,0);
    assert(listener>=0);
    sockaddr_un address{};address.sun_family=AF_UNIX;
    std::memcpy(address.sun_path,path.c_str(),path.size()+1);
    assert(bind(listener,reinterpret_cast<sockaddr*>(&address),sizeof(address))==0);
    assert(chmod(path.c_str(),0600)==0);
    assert(listen(listener,2)==0);
    std::atomic<bool> secondDone{false};
    std::jthread server([&] {
        uint64_t previous=0;
        for (unsigned round=0;round<2;++round) {
            const int client=accept4(listener,nullptr,nullptr,SOCK_CLOEXEC);
            assert(client>=0);
            WirePacket packet{};
            assert(recv(client,packet.data(),512,MSG_TRUNC)==512);
            const auto nonce=wire::get(packet,48,8);
            assert(nonce>previous);previous=nonce;
            wire::put(packet,6,2,2);
            assert(send(client,packet.data(),512,MSG_NOSIGNAL)==512);
            if (round==0) {
                std::this_thread::sleep_for(std::chrono::milliseconds(30));
            } else {
                assert(recv(client,packet.data(),512,MSG_TRUNC)==512);
                assert(wire::get(packet,48,8)==nonce && wire::get(packet,64,4)==8);
                secondDone.store(true);
                // A stale mailbox Event must close, never be written here.
                assert(recv(client,packet.data(),512,MSG_TRUNC)==0);
            }
            close(client);
        }
    });
    DecisionMailbox mailbox;
    WirePacket hello{};assert(wire::hello(hello,100,"sdk-hash","closure+digest"));
    {
        StockWorker worker(mailbox,path,hello);
        const auto limit=std::chrono::steady_clock::now()+std::chrono::seconds(3);
        while ((!worker.connected() || worker.generation()<2) && std::chrono::steady_clock::now()<limit)
            std::this_thread::yield();
        assert(worker.connected() && worker.generation()==2 && worker.nonce()==101);
        auto boundary=hello;
        for (unsigned i=64;i<512;++i) boundary[i]=0;
        wire::put(boundary,6,2,7);wire::put(boundary,8,4,8);
        wire::put(boundary,40,8,monotonicNs()+10'000'000);wire::put(boundary,64,4,8);
        assert(!worker.notify(boundary)); // previous nonce cannot reach fresh peer
        wire::put(boundary,48,8,worker.nonce());
        assert(worker.notify(boundary));
        while (!secondDone.load() && std::chrono::steady_clock::now()<limit) std::this_thread::yield();
        assert(secondDone.load());
        assert(worker.reasonCount(StockWorker::Reason::Authenticated)==2);
        assert(worker.reasonCount(StockWorker::Reason::IdleHangup)==1);
        auto stale=hello;wire::put(stale,6,2,3);wire::put(stale,8,4,32);
        for (unsigned i=64;i<512;++i) stale[i]=0;
        const auto deadline=monotonicNs()+10'000'000;
        wire::put(stale,40,8,deadline);
        WirePacket reply{};
        (void)mailbox.decide(stale,deadline,reply);
        assert(!wire::decision(stale,reply));
    }
    server.join();close(listener);
    assert(unlink(path.c_str())==0);assert(rmdir(directory)==0);
}
}
int main() { framing(); connection(); delayedHello(); reconnect(); disconnectedDrain(); }
