// SPDX-License-Identifier: Apache-2.0
// Standalone allocation, containment and leak regressions. No allocator overrides
// are linked into the production shim. Include the implementation so `direct`
// checks real destruction with LSan; worker exit must never hide a leak.
// Build from the repository root:
// nice -n 10 c++ -std=c++17 -O1 -g -ffp-contract=off -fno-fast-math -DNDEBUG \
//   -DNOMINMAX -DSM_GRAPHICS_CALLBACKS=1 -Iharness/cpp/shim \
//   -Iupstream/source/SMLib/inc -Iupstream/source/SM_API/inc \
//   harness/cpp/tests/blend_fillet_solver_p5_alloc.cpp \
//   harness/cpp/shim/topology_edgeuse_p1.cpp harness/cpp/shim/sc_shim.cpp \
//   -Wl,--wrap=calloc \
//   -L"${SOLIDCORE_CPP_DIR:-bin/cpp}/lib" -lSM_API -lSMLib -pthread \
//   -o bin/blend_fillet_solver_p5_alloc
// Add -fsanitize=address -fno-omit-frame-pointer for `direct` leak checks.
// nice -n 10 bin/blend_fillet_solver_p5_alloc direct 0
// nice -n 10 bin/blend_fillet_solver_p5_alloc direct 2
// nice -n 10 bin/blend_fillet_solver_p5_alloc new 0   # failure index 0..299
// nice -n 10 bin/blend_fillet_solver_p5_alloc calloc 91
// On macOS omit --wrap=calloc: calloc is overridden via malloc_zone_calloc.
#include <cstdio>
#include <cstdlib>
#include <new>
#include <cstring>
#if defined(__APPLE__)
#include <malloc/malloc.h>
#endif
static long new_failure=-1,calloc_failure=-1;
static bool test_exit_trap=false;
struct ExitTrap { ~ExitTrap() { if(test_exit_trap) std::abort(); } };
static ExitTrap exit_trap;
void* operator new(std::size_t size) {
    if(new_failure==0) {new_failure=-1; throw std::bad_alloc();}
    if(new_failure>0) --new_failure;
    if(void* p=std::malloc(size?size:1)) return p;
    throw std::bad_alloc();
}
void operator delete(void* p) noexcept {std::free(p);}
void operator delete(void* p,std::size_t) noexcept {std::free(p);}
#if defined(__APPLE__)
extern "C" void* calloc(std::size_t n,std::size_t size) {
#else
extern "C" void* __real_calloc(std::size_t,std::size_t);
extern "C" void* __wrap_calloc(std::size_t n,std::size_t size) {
#endif
    if(calloc_failure==0) {calloc_failure=-1; return nullptr;}
    if(calloc_failure>0) --calloc_failure;
#if defined(__APPLE__)
    return ::malloc_zone_calloc(::malloc_default_zone(),n,size);
#else
    return __real_calloc(n,size);
#endif
}
static const char* worker_executable=nullptr;
#define SC_ROLLOVER_FAULT_TEST
#define SC_ROLLOVER_WORKER_PATH worker_executable
#include "../worker/rollover.cpp"
int main(int argc,char**argv) {
    if(argc==2 && std::strcmp(argv[1],"--rollover-worker")==0) std::_Exit(rollover_worker_main());
    if(argc<3) return 2;
    worker_executable=argv[0]; // runner passes an absolute path on both hosts
    const long value=std::strtol(argv[2],nullptr,10);
    if(std::strcmp(argv[1],"--descriptor-worker")==0)
        std::_Exit(::fcntl(static_cast<int>(value),F_GETFD)==-1 && errno==EBADF?0:3);
    if(std::strcmp(argv[1],"descriptor")==0) {
        int fds[2];
        if(::pipe(fds)!=0) return 3;
#if defined(__linux__)
        ::fcntl(fds[0],F_SETFD,FD_CLOEXEC);
        ::fcntl(fds[1],F_SETFD,FD_CLOEXEC);
#endif
        // On Darwin intentionally leave this pipe unmarked, simulating another
        // thread between pipe() and fcntl(). The spawned worker must close it.
        char descriptor[32];std::snprintf(descriptor,sizeof(descriptor),"%d",fds[0]);
        char* args[]={argv[0],const_cast<char*>("--descriptor-worker"),descriptor,nullptr};
        posix_spawn_file_actions_t actions;
        if(::posix_spawn_file_actions_init(&actions)!=0) {::close(fds[0]);::close(fds[1]);return 3;}
        pid_t child=-1;
        int error=sc_rollover::spawn_worker(&child,worker_executable,&actions,args);
        ::posix_spawn_file_actions_destroy(&actions);
        ::close(fds[0]);::close(fds[1]);
        int status=0;pid_t reaped=-1;
        if(error==0) do {reaped=::waitpid(child,&status,0);} while(reaped<0 && errno==EINTR);
        if(error!=0 || reaped!=child || !WIFEXITED(status) || WEXITSTATUS(status)!=0) return 3;
        std::printf("DESCRIPTOR PASS: unrelated pipe not inherited\n");
        return 0;
    }
    double input[]={0,0,-1,0,1,0,1,102,1,0,1,103};
    double out[128]; int64_t len=0;
    if(std::strcmp(argv[1],"direct-target")==0) {
        double target[4096];
        int status=rollover_target_in_process(target,4096,&len);
        std::printf("direct target status=%d len=%lld\n",status,(long long)len);
        return status==SC_OK?0:3;
    }
    if(std::strcmp(argv[1],"guard")==0) {
        SmContext ctx;
        SmFilletBrep pseudo;
        SmFilletEdge a(&pseudo),b(&pseudo);
        Solver solver(ctx);
        SmFilletGeom* geom=solver.geom();
        geom->SetRail(0,&a);geom->SetRail(1,&b);
        auto* ca=new(ctx) SmCurveClassification();ca->SetSize(2);a.SetCurveClass(ca);
        auto* cb=new(ctx) SmCurveClassification();cb->SetSize(2);b.SetCurveClass(cb);
        if(!safe_process_rail_copies(geom)) return 3; // NER on null surface
        geom->SetFilletSurface(new(ctx) SmPlane(SmPoint3d(0,0,0),SmVector3d(0,0,1),&ctx));
        if(safe_process_rail_copies(geom)) return 3; // would dereference null curve
        ca->SetSize(1);cb->SetSize(1);
        if(!safe_process_rail_copies(geom)) return 3; // defined 1x1 early return
        ca->SetSize(2);geom->SetFilletGeomType(SM_FG_BLENDS);
        if(!safe_process_rail_copies(geom)) return 3; // blend early return
        std::printf("GUARD PASS: branch-specific null curve check\n");
        return 0;
    }
    if(std::strcmp(argv[1],"exit")==0) {
        sc_rollover::test_signal=2; // arm a failing global destructor after the reply
        int status=sc_rollover::isolated(0,input,12,out,128,&len);
        std::printf("post-response global destructor skipped: status=%d\n",status);
        return status==SC_OK?0:3;
    }
    if(std::strcmp(argv[1],"signal")==0) {
        sc_rollover::test_signal=value;
        int status=sc_rollover::isolated(0,input,12,out,128,&len);
        std::printf("worker signal contained: status=%d\n",status);
        return status==SC_ERR_EXCEPTION?0:3;
    }
    if(std::strcmp(argv[1],"direct")==0) {
        input[0]=value;
        int status=rollover_eval_in_process(input,12,out,128,&len);
        std::printf("direct op=%ld status=%d len=%lld\n",value,status,(long long)len);
        return status==SC_OK?0:3;
    }
    bool target=argc>3 && std::strcmp(argv[3],"target")==0;
    if(std::strcmp(argv[1],"new")==0) sc_rollover::fail_new=value;
    else if(std::strcmp(argv[1],"calloc")==0) sc_rollover::fail_calloc=value;
    else return 2;
    double target_out[4096];
    int status=target?sc_blend_fillet_solver_p5_target(target_out,4096,&len)
        :sc_blend_fillet_solver_p5_eval(input,12,out,128,&len);
    new_failure=calloc_failure=-1;
    std::printf("contained %s index=%ld target=%d status=%d\n",argv[1],value,target,status);
    return status==SC_OK || status==SC_ERR_EXCEPTION || status==SC_ERR_ARG?0:4;
}
